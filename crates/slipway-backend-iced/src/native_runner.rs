use super::*;

use iced::advanced::Renderer as _;
use iced::advanced::graphics::Compositor as _;
use iced::advanced::graphics::Viewport;
use iced::advanced::graphics::compositor;
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Renderer as _};
use iced::theme::Base as _;
use iced_core::input_method;
use iced_winit::runtime::user_interface::{self, UserInterface};
use iced_winit::winit;
use slipway_core::{
    BackendInputTrace, InputEvent, PointerButton as DebugPointerButton, PresentationRegionId, Size,
    TextCompositionEvent, TextCompositionPhase, WidgetId,
};
use slipway_debug_bridge::{
    CompositionPhaseProvenance, DEBUG_COMPOSITION_PASS_ID, DebugCommandLease,
    DebugCompositionCommitMutation, DebugCompositionIngressObservation, DebugCompositionPhaseTrace,
    DebugCompositionTrace, DebugControlMode, DebugControlTrace, DebugFailure, DebugPhysicalControl,
    DebugReplyProduct, PresentedAlphaMode, PresentedCapturePath, PresentedPixels,
    PresentedScreenshotProduct, PresentedScreenshotRefusal, PresentedScreenshotSelector,
    PresentedSurfaceFormat, PresentedTransferFunction, VISIBLE_FRAME_BUDGET_NS,
    VisibleFrameTimingRecorder,
};
use slipway_runtime::{SlipwayImePolicy, SlipwayRuntimePendingNativeMcpCall};
use std::borrow::Cow;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{
    DeviceId, ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{
    ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle,
};
use winit::keyboard::ModifiersState;
use winit::window::{ImePurpose, Window, WindowId};

#[derive(Clone, Copy, Debug)]
enum NativeRunnerEvent {
    McpWake,
    DirectCaptureWake { token: u64 },
}

fn spawn_iced_capture_deadline(
    proxy: EventLoopProxy<NativeRunnerEvent>,
    token: u64,
    delay: Duration,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("slipway-iced-direct-capture-deadline".to_string())
        .spawn(move || {
            std::thread::sleep(delay);
            let _ = proxy.send_event(NativeRunnerEvent::DirectCaptureWake { token });
        })
}

fn ingress_token(origin: &IcedIngressOrigin) -> u64 {
    match origin {
        IcedIngressOrigin::NativeOs => 0,
        IcedIngressOrigin::Debug { token, .. }
        | IcedIngressOrigin::DebugComposition { token, .. } => *token,
    }
}

fn retag_backend_input(input: &mut BackendInputEvent, pass_id: &str) {
    if let Some(evidence) = input.dispatch_evidence.as_mut() {
        evidence.source = slipway_core::EvidenceSource::backend_presented(ICED_BACKEND_ID, pass_id);
    }
}

fn retag_iced_ingress_message(
    message: SlipwayIcedRuntimeMessage,
    pass_id: &str,
) -> SlipwayIcedRuntimeMessage {
    match message {
        SlipwayIcedRuntimeMessage::BackendInput(mut input) => {
            retag_backend_input(&mut input, pass_id);
            SlipwayIcedRuntimeMessage::BackendInput(input)
        }
        SlipwayIcedRuntimeMessage::NativeTextMutation(mut mutation) => {
            retag_backend_input(&mut mutation.input, pass_id);
            SlipwayIcedRuntimeMessage::NativeTextMutation(mutation)
        }
        other => other,
    }
}

fn composition_backend_event_label(phase: TextCompositionPhase) -> &'static str {
    match phase {
        TextCompositionPhase::Start => "winit::Ime::Enabled",
        TextCompositionPhase::Update => "winit::Ime::Preedit",
        TextCompositionPhase::Commit => "winit::Ime::Commit",
        TextCompositionPhase::End => "winit::Ime::Disabled",
        TextCompositionPhase::Cancel => "winit::Ime::Cancelled",
    }
}

fn unicode_scalar_to_byte_index(text: &str, scalar_index: usize) -> usize {
    text.char_indices()
        .nth(scalar_index)
        .map_or(text.len(), |(index, _)| index)
}

fn direct_capture_refusal(
    reason: iced_renderer::wgpu::window::compositor::DirectCaptureRefusal,
) -> (&'static str, &'static str) {
    use iced_renderer::wgpu::window::compositor::DirectCaptureRefusal;
    match reason {
        DirectCaptureRefusal::CopySrcUnsupported => (
            "screenshot-copy-src-unsupported",
            "the selected iced wgpu surface does not support COPY_SRC usage",
        ),
        DirectCaptureRefusal::FormatUnsupported => (
            "screenshot-format-unsupported",
            "the selected iced wgpu surface format cannot be normalized to RGBA8",
        ),
        DirectCaptureRefusal::AlphaUnsupported => (
            "screenshot-alpha-unsupported",
            "the selected iced wgpu surface alpha mode is unsupported",
        ),
        DirectCaptureRefusal::ZeroSize => (
            "screenshot-zero-size",
            "the selected iced wgpu surface has zero width or height",
        ),
        DirectCaptureRefusal::ViewportUnavailable => (
            "screenshot-viewport-unavailable",
            "the iced wgpu viewport is unavailable for direct capture",
        ),
        DirectCaptureRefusal::AlreadyArmed => (
            "screenshot-already-armed",
            "the iced wgpu surface already has a direct capture request",
        ),
    }
}

fn presented_surface_format(
    format: iced_renderer::wgpu::window::compositor::DirectCaptureFormat,
) -> PresentedSurfaceFormat {
    use iced_renderer::wgpu::window::compositor::DirectCaptureFormat;
    match format {
        DirectCaptureFormat::Rgba8Unorm => PresentedSurfaceFormat::Rgba8Unorm,
        DirectCaptureFormat::Rgba8UnormSrgb => PresentedSurfaceFormat::Rgba8UnormSrgb,
        DirectCaptureFormat::Bgra8Unorm => PresentedSurfaceFormat::Bgra8Unorm,
        DirectCaptureFormat::Bgra8UnormSrgb => PresentedSurfaceFormat::Bgra8UnormSrgb,
    }
}

fn presented_transfer(
    format: iced_renderer::wgpu::window::compositor::DirectCaptureFormat,
) -> PresentedTransferFunction {
    use iced_renderer::wgpu::window::compositor::DirectCaptureFormat;
    match format {
        DirectCaptureFormat::Rgba8Unorm | DirectCaptureFormat::Bgra8Unorm => {
            PresentedTransferFunction::Linear
        }
        DirectCaptureFormat::Rgba8UnormSrgb | DirectCaptureFormat::Bgra8UnormSrgb => {
            PresentedTransferFunction::Srgb
        }
    }
}

fn presented_alpha(
    alpha: iced_renderer::wgpu::window::compositor::DirectCaptureAlphaMode,
) -> PresentedAlphaMode {
    use iced_renderer::wgpu::window::compositor::DirectCaptureAlphaMode;
    match alpha {
        DirectCaptureAlphaMode::Opaque => PresentedAlphaMode::Opaque,
        DirectCaptureAlphaMode::Premultiplied => PresentedAlphaMode::Premultiplied,
    }
}

fn composition_event_for_command(
    command: &DebugCommand,
    sequence_index: usize,
    target: &WidgetId,
    target_slot: Option<&slipway_core::WidgetSlotAddress>,
) -> Option<TextCompositionEvent> {
    let DebugCommandKind::PhysicalControl {
        operation:
            DebugPhysicalControl::TextComposition {
                selector: _,
                updates,
                commit,
            },
        ..
    } = &command.kind
    else {
        return None;
    };
    let (phase, preedit_text, cursor_range) = if sequence_index == 0 {
        (TextCompositionPhase::Start, String::new(), None)
    } else if sequence_index <= updates.len() {
        let update = &updates[sequence_index - 1];
        (
            TextCompositionPhase::Update,
            update.preedit_text.clone(),
            update.cursor_range.clone(),
        )
    } else if sequence_index == updates.len() + 1 {
        (TextCompositionPhase::Commit, commit.clone(), None)
    } else {
        (TextCompositionPhase::End, String::new(), None)
    };
    Some(TextCompositionEvent {
        target: target.clone(),
        target_slot: target_slot.cloned(),
        phase,
        preedit_text,
        cursor_range,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum IcedIngressOrigin {
    NativeOs,
    Debug {
        token: u64,
        final_in_plan: bool,
    },
    DebugComposition {
        token: u64,
        sequence_index: usize,
        phase: TextCompositionPhase,
    },
}

#[derive(Debug)]
struct QueuedIcedIngress {
    event: iced::Event,
    origin: IcedIngressOrigin,
    commit_mutation_gate: Option<IcedNativeCommitMutationGate>,
}

#[derive(Debug)]
struct QueuedIcedDispatchSlice {
    events: Vec<iced::Event>,
    origin: IcedIngressOrigin,
    commit_mutation_gate: Option<IcedNativeCommitMutationGate>,
}

struct PendingIcedNativePhysicalControl {
    token: u64,
    command: DebugCommand,
    lease: DebugCommandLease,
    pending: SlipwayRuntimePendingNativeMcpCall,
    evidence: PendingIcedPhysicalEvidence,
}

enum PendingIcedPhysicalEvidence {
    Ordinary {
        matched_trace: Option<BackendInputTrace>,
        last_refusal: Option<slipway_core::DeclaredEventDispatchEvidence>,
    },
    Composition {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
        region: PresentationRegionId,
        focused_before: bool,
        phases: Vec<DebugCompositionPhaseTrace>,
        commit_mutation: Option<DebugCompositionCommitMutation>,
    },
}

struct DirectCaptureWakeProxy {
    proxy: EventLoopProxy<NativeRunnerEvent>,
}

impl iced_renderer::wgpu::window::compositor::DirectCaptureWake for DirectCaptureWakeProxy {
    fn wake(&self, token: u64) {
        let _ = self
            .proxy
            .send_event(NativeRunnerEvent::DirectCaptureWake { token });
    }
}

struct CaptureLease {
    token: u64,
    command: DebugCommand,
    lease: DebugCommandLease,
    pending: SlipwayRuntimePendingNativeMcpCall,
    event_rx: Receiver<iced_renderer::wgpu::window::compositor::DirectCaptureEvent>,
    event_tx: SyncSender<iced_renderer::wgpu::window::compositor::DirectCaptureEvent>,
    wake: Arc<dyn iced_renderer::wgpu::window::compositor::DirectCaptureWake>,
    deadline: Instant,
}

#[allow(clippy::too_many_arguments)]
fn build_iced_capture_lease<S>(
    token: u64,
    command: DebugCommand,
    lease: DebugCommandLease,
    pending: SlipwayRuntimePendingNativeMcpCall,
    event_rx: Receiver<iced_renderer::wgpu::window::compositor::DirectCaptureEvent>,
    event_tx: SyncSender<iced_renderer::wgpu::window::compositor::DirectCaptureEvent>,
    wake: Arc<dyn iced_renderer::wgpu::window::compositor::DirectCaptureWake>,
    delay: Duration,
    spawn_deadline: S,
) -> Result<
    CaptureLease,
    (
        DebugCommand,
        DebugCommandLease,
        SlipwayRuntimePendingNativeMcpCall,
    ),
>
where
    S: FnOnce(Duration) -> std::io::Result<std::thread::JoinHandle<()>>,
{
    let deadline = Instant::now() + delay;
    if spawn_deadline(delay).is_err() {
        return Err((command, lease, pending));
    }
    Ok(CaptureLease {
        token,
        command,
        lease,
        pending,
        event_rx,
        event_tx,
        wake,
        deadline,
    })
}

fn complete_presented_capture_refusal(
    command: DebugCommand,
    lease: DebugCommandLease,
    pending: SlipwayRuntimePendingNativeMcpCall,
    captured_frame: Option<FrameIdentity>,
    code: &str,
    reason: &str,
) {
    let selector = screenshot_selector(&command).clone();
    let product = DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Refusal(
        PresentedScreenshotRefusal {
            selector,
            captured_frame,
            backend_id: Some(ICED_BACKEND_ID.to_string()),
            code: code.to_string(),
            reason: reason.to_string(),
            diagnostics: Vec::new(),
        },
    ));
    let _ = lease.complete(product);
    let _ = pending.try_finish_and_respond();
}

struct PresentedMeta {
    format: iced_renderer::wgpu::window::compositor::DirectCaptureFormat,
    alpha: iced_renderer::wgpu::window::compositor::DirectCaptureAlphaMode,
    width: u32,
    height: u32,
}

#[derive(Default)]
struct IcedCaptureEvidence {
    presented: Option<PresentedMeta>,
    mapped: Option<Arc<[u8]>>,
}

fn iced_capture_deadline_refusal(
    committed_frame: Option<&FrameIdentity>,
) -> (Option<FrameIdentity>, &'static str, &'static str) {
    (
        committed_frame.cloned(),
        "screenshot-deadline",
        "the iced direct presented capture did not complete before its request deadline",
    )
}

fn iced_capture_teardown_refusal(
    committed_frame: Option<&FrameIdentity>,
) -> (Option<FrameIdentity>, &'static str, &'static str) {
    (
        committed_frame.cloned(),
        "screenshot-window-teardown",
        "the iced visible window closed before direct presented capture completed",
    )
}

enum IcedCaptureSignal {
    Presented {
        format: iced_renderer::wgpu::window::compositor::DirectCaptureFormat,
        alpha: iced_renderer::wgpu::window::compositor::DirectCaptureAlphaMode,
        width: u32,
        height: u32,
    },
    Mapped(Arc<[u8]>),
    MapFailed,
    PollFailed,
    Refused {
        code: &'static str,
        reason: &'static str,
    },
}

enum IcedCaptureReduction {
    Continue,
    Refused {
        code: &'static str,
        reason: &'static str,
    },
}

fn reduce_iced_capture_signal(
    _committed_frame: Option<&FrameIdentity>,
    evidence: &mut IcedCaptureEvidence,
    signal: IcedCaptureSignal,
) -> IcedCaptureReduction {
    match signal {
        IcedCaptureSignal::Presented {
            format,
            alpha,
            width,
            height,
        } if evidence.presented.is_none() => {
            evidence.presented = Some(PresentedMeta {
                format,
                alpha,
                width,
                height,
            });
        }
        IcedCaptureSignal::Presented { .. } => {}
        IcedCaptureSignal::Mapped(bytes) if evidence.mapped.is_none() => {
            evidence.mapped = Some(bytes);
        }
        IcedCaptureSignal::Mapped(_) => {}
        IcedCaptureSignal::MapFailed if evidence.mapped.is_none() => {
            return IcedCaptureReduction::Refused {
                code: "screenshot-map-failed",
                reason: "the iced direct surface staging buffer could not be mapped",
            };
        }
        IcedCaptureSignal::MapFailed => {}
        IcedCaptureSignal::PollFailed if evidence.mapped.is_none() => {
            return IcedCaptureReduction::Refused {
                code: "screenshot-poll-failed",
                reason: "the iced direct surface readback device poll failed or timed out",
            };
        }
        IcedCaptureSignal::PollFailed => {}
        IcedCaptureSignal::Refused { code, reason } => {
            return IcedCaptureReduction::Refused { code, reason };
        }
    }
    IcedCaptureReduction::Continue
}

fn reduce_iced_capture_event(
    expected_token: u64,
    committed_frame: Option<&FrameIdentity>,
    evidence: &mut IcedCaptureEvidence,
    event: iced_renderer::wgpu::window::compositor::DirectCaptureEvent,
) -> IcedCaptureReduction {
    use iced_renderer::wgpu::window::compositor::DirectCaptureEvent;
    let event_token = match &event {
        DirectCaptureEvent::Presented { token, .. }
        | DirectCaptureEvent::Mapped { token, .. }
        | DirectCaptureEvent::PollFailed { token, .. }
        | DirectCaptureEvent::Refused { token, .. } => *token,
    };
    if event_token != expected_token {
        return IcedCaptureReduction::Continue;
    }
    let signal = match event {
        DirectCaptureEvent::Presented {
            format,
            alpha,
            width,
            height,
            ..
        } => IcedCaptureSignal::Presented {
            format,
            alpha,
            width,
            height,
        },
        DirectCaptureEvent::Mapped {
            result: Ok(bytes), ..
        } => IcedCaptureSignal::Mapped(bytes),
        DirectCaptureEvent::Mapped { result: Err(_), .. } => IcedCaptureSignal::MapFailed,
        DirectCaptureEvent::PollFailed { .. } => IcedCaptureSignal::PollFailed,
        DirectCaptureEvent::Refused { reason, .. } => {
            let (code, reason) = direct_capture_refusal(reason);
            IcedCaptureSignal::Refused { code, reason }
        }
    };
    reduce_iced_capture_signal(committed_frame, evidence, signal)
}

enum PendingPresentedCapture {
    Armed(CaptureLease),
    Waiting {
        lease: CaptureLease,
        committed_frame: FrameIdentity,
        evidence: IcedCaptureEvidence,
    },
}

struct IcedPresentOutcome {
    result: IcedPresentationResult,
    pending_capture: Option<PendingPresentedCapture>,
    capture_failure: Option<(CaptureLease, &'static str, &'static str)>,
}

enum IcedPresentationResult {
    SkippedOverflow,
    CompositorFailed(compositor::SurfaceError),
    Presented,
}

fn screenshot_selector(command: &DebugCommand) -> &PresentedScreenshotSelector {
    match &command.kind {
        DebugCommandKind::Screenshot { request } => &request.selector,
        _ => unreachable!("presented capture lease must own a screenshot command"),
    }
}

const RESIZE_CONFIGURE_QUIET_FRAMES: u64 = 12;
const RESIZE_CONFIGURE_QUIET: Duration =
    Duration::from_nanos((VISIBLE_FRAME_BUDGET_NS as u64) * RESIZE_CONFIGURE_QUIET_FRAMES);

pub fn run_slipway_iced_runtime_app_native<W, F>(
    widget: W,
    external: W::ExternalState,
    apply_app_messages: F,
    config: SlipwayRuntimeConfig,
) -> iced::Result
where
    W: SlipwayIcedBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: Clone + std::fmt::Debug + Send + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    let ime_policy = config.ime_policy;
    let assembled = SlipwayAssembledApp::with_config(widget, external, config);
    let debug_mcp_transport = assembled
        .runtime
        .start_debug_mcp_transport()
        .map_err(|error| iced::Error::WindowCreationFailed(Box::new(error)))?;
    let app = SlipwayIcedRuntimeApp::new(assembled, apply_app_messages)
        .with_debug_mcp_transport(debug_mcp_transport);

    let event_loop = EventLoop::<NativeRunnerEvent>::with_user_event()
        .build()
        .map_err(|error| iced::Error::WindowCreationFailed(Box::new(error)))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let display_handle = event_loop.owned_display_handle();
    let proxy = event_loop.create_proxy();
    let ime_trace = std::env::var_os("SLIPWAY_IME_TRACE").is_some();
    let mut runner = NativeIcedRunner::new(app, display_handle, proxy, ime_policy, ime_trace);

    event_loop
        .run_app(&mut runner)
        .map_err(|error| iced::Error::WindowCreationFailed(Box::new(error)))
}

struct NativeIcedRunner<W, F>
where
    W: SlipwayIcedBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: Clone + std::fmt::Debug + Send + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    app: SlipwayIcedRuntimeApp<W, F>,
    display_handle: OwnedDisplayHandle,
    proxy: EventLoopProxy<NativeRunnerEvent>,
    ime_policy: SlipwayImePolicy,
    ime_trace: bool,
    window: Option<NativeIcedWindow>,
    pending_mcp: Option<SlipwayRuntimePendingNativeMcpCall>,
    pending_iced_events: Vec<QueuedIcedIngress>,
    pending_physical_control: Option<PendingIcedNativePhysicalControl>,
    pending_presented_capture: Option<PendingPresentedCapture>,
    backend_present_index: u64,
    last_successfully_presented: Option<FrameIdentity>,
    next_debug_token: u64,
    pending_resize: Option<PendingResize>,
    frame_timing: VisibleFrameTimingRecorder,
}

struct PendingResize {
    physical_size: PhysicalSize<u32>,
    queued_at: Instant,
}

struct NativeIcedWindow {
    raw: Arc<Window>,
    compositor: iced_renderer::Compositor,
    surface: <iced_renderer::Compositor as iced::advanced::graphics::Compositor>::Surface,
    surface_physical_size: PhysicalSize<u32>,
    renderer: iced::Renderer,
    cache: user_interface::Cache,
    clipboard: iced_winit::Clipboard,
    viewport: Viewport,
    cursor_position: Option<PhysicalPosition<f64>>,
    modifiers: ModifiersState,
    mouse_interaction: mouse::Interaction,
    ime_state: Option<(iced::Rectangle, iced_core::input_method::Purpose)>,
    preedit: Option<NativePreedit>,
    preedit_text_color: Option<iced::Color>,
    preedit_font: Option<iced::Font>,
    preedit_text_size: Option<f32>,
    ime_composing: bool,
    ime_allowed: bool,
    ime_policy: SlipwayImePolicy,
    ime_trace: bool,
    redraw_at: Option<Instant>,
    presented_frames: u64,
    pressed_mouse_buttons: u8,
    theme: iced::Theme,
    style: renderer::Style,
}

struct NativePreedit {
    cursor: iced::Rectangle,
    content: <iced::Renderer as text::Renderer>::Paragraph,
    spans: Vec<text::Span<'static, (), iced::Font>>,
}

impl NativePreedit {
    fn new() -> Self {
        Self {
            cursor: iced::Rectangle::default(),
            content: Default::default(),
            spans: Vec::new(),
        }
    }

    fn update(
        &mut self,
        cursor: iced::Rectangle,
        preedit: &input_method::Preedit,
        text_color: iced::Color,
        font: iced::Font,
        text_size: f32,
        _renderer: &iced::Renderer,
    ) {
        self.cursor = cursor;

        let spans = match &preedit.selection {
            Some(selection) => vec![
                text::Span::new(&preedit.content[..selection.start]).color(text_color),
                text::Span::new(if selection.start == selection.end {
                    "\u{200A}"
                } else {
                    &preedit.content[selection.start..selection.end]
                })
                .color(text_color),
                text::Span::new(&preedit.content[selection.end..]).color(text_color),
            ],
            None => vec![text::Span::new(&preedit.content).color(text_color)],
        };

        if spans != self.spans.as_slice() {
            self.content =
                <iced::Renderer as text::Renderer>::Paragraph::with_spans(iced_core::Text {
                    content: &spans,
                    bounds: iced::Size::INFINITE,
                    size: iced::Pixels(preedit.text_size.map_or(text_size, |size| size.0)),
                    line_height: text::LineHeight::default(),
                    font,
                    align_x: text::Alignment::Default,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::None,
                });

            self.spans.clear();
            self.spans
                .extend(spans.into_iter().map(text::Span::to_static));
        }
    }

    fn draw(&self, renderer: &mut iced::Renderer, color: iced::Color, viewport: &iced::Rectangle) {
        if self.content.min_width() < 1.0 {
            return;
        }

        let content_bounds = self.content.min_bounds();
        let vertical_padding = (self.cursor.height - content_bounds.height).max(0.0) / 2.0;
        let mut bounds = iced::Rectangle::new(
            iced::Point::new(self.cursor.x, self.cursor.y + vertical_padding),
            content_bounds,
        );

        bounds.x = bounds
            .x
            .max(viewport.x)
            .min(viewport.x + viewport.width - bounds.width);
        bounds.y = bounds
            .y
            .max(viewport.y)
            .min(viewport.y + viewport.height - bounds.height);

        renderer.with_layer(bounds, |renderer| {
            renderer.fill_paragraph(&self.content, bounds.position(), color, bounds);

            const UNDERLINE: f32 = 1.0;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: bounds.shrink(iced::Padding {
                        top: bounds.height - UNDERLINE,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                color,
            );

            for span_bounds in self.content.span_bounds(1) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: span_bounds + (bounds.position() - iced::Point::ORIGIN),
                        ..Default::default()
                    },
                    color,
                );
            }
        });
    }
}

impl<W, F> NativeIcedRunner<W, F>
where
    W: SlipwayIcedBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: Clone + std::fmt::Debug + Send + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    fn new(
        app: SlipwayIcedRuntimeApp<W, F>,
        display_handle: OwnedDisplayHandle,
        proxy: EventLoopProxy<NativeRunnerEvent>,
        ime_policy: SlipwayImePolicy,
        ime_trace: bool,
    ) -> Self {
        let backend_present_index = app.runtime().last_frame_identity().frame_index;
        Self {
            app,
            display_handle,
            proxy,
            ime_policy,
            ime_trace,
            window: None,
            pending_mcp: None,
            pending_iced_events: Vec::new(),
            pending_physical_control: None,
            pending_presented_capture: None,
            backend_present_index,
            last_successfully_presented: None,
            next_debug_token: 1,
            pending_resize: None,
            frame_timing: VisibleFrameTimingRecorder::from_env("iced"),
        }
    }

    fn start_wake_thread(&self) {
        let Some(transport) = self.app.debug_mcp_transport.as_ref() else {
            return;
        };
        let wake_rx = transport.wake_receiver();
        let proxy = self.proxy.clone();
        let _ = std::thread::Builder::new()
            .name("slipway-iced-native-mcp-wake".to_string())
            .spawn(move || {
                while wake_rx.recv() {
                    if proxy.send_event(NativeRunnerEvent::McpWake).is_err() {
                        break;
                    }
                }
            });
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), iced::Error> {
        if self.window.is_some() {
            return Ok(());
        }

        let create_window_start = Instant::now();
        let mut window_settings = iced::window::Settings::default();
        window_settings.size = iced::Size::new(1024.0, 768.0);
        // Program-level zoom for `window_attributes`, not monitor DPI:
        // iced_winit passes `Program::scale_factor` (default 1.0) here and
        // winit itself applies the monitor DPI to the logical size.
        let program_scale_factor = 1.0;
        let title = self.app.title();
        let window_start = Instant::now();
        let window = Arc::new(
            event_loop
                .create_window(iced_winit::conversion::window_attributes(
                    window_settings,
                    &title,
                    program_scale_factor,
                    None,
                    None,
                ))
                .map_err(|error| iced::Error::WindowCreationFailed(Box::new(error)))?,
        );
        // Windows emits ScaleFactorChanged only on DPI *changes* after
        // creation, so the initial viewport must read the created window's
        // scale factor the same way `queue_resize` does (f32 cast parity).
        let scale_factor = window.scale_factor() as f32;
        self.frame_timing
            .record("iced.create_window.window", window_start.elapsed(), 1, None);
        window.set_title(&title);
        let ime_allowed = self.ime_policy.keeps_platform_ime_allowed();
        if ime_allowed {
            window.set_ime_allowed(true);
            window.set_ime_cursor_area(LogicalPosition::new(0.0, 0.0), LogicalSize::new(1.0, 1.0));
            window.set_ime_purpose(ImePurpose::Normal);
            trace_ime(
                self.ime_trace,
                "window-create set_ime_allowed(true) cursor_area=(0,0,1,1)",
            );
        }

        let executor_start = Instant::now();
        let executor = <iced::executor::Default as iced::Executor>::new()
            .map_err(iced::Error::ExecutorCreationFailed)?;
        self.frame_timing.record(
            "iced.create_window.executor",
            executor_start.elapsed(),
            1,
            None,
        );
        let mut graphics_settings: iced::advanced::graphics::Settings =
            iced::Settings::default().into();
        graphics_settings.vsync = false;
        let compositor_start = Instant::now();
        let mut compositor = executor
            .block_on(
                <iced_renderer::Compositor as iced::advanced::graphics::Compositor>::new(
                    graphics_settings,
                    self.display_handle.clone(),
                    window.clone(),
                    iced::advanced::graphics::Shell::headless(),
                ),
            )
            .map_err(iced::Error::GraphicsCreationFailed)?;
        self.frame_timing.record(
            "iced.create_window.compositor",
            compositor_start.elapsed(),
            1,
            None,
        );
        let font_start = Instant::now();
        let loaded_platform_fonts =
            load_platform_text_fonts(&mut compositor, self.ime_trace, &mut self.frame_timing);
        self.frame_timing.record(
            "iced.create_window.font",
            font_start.elapsed(),
            loaded_platform_fonts,
            None,
        );
        let physical_size = non_zero_physical_size(window.inner_size());
        let timing_physical_size = Some(Size {
            width: physical_size.width as f32,
            height: physical_size.height as f32,
        });
        let surface_start = Instant::now();
        let surface =
            compositor.create_surface(window.clone(), physical_size.width, physical_size.height);
        self.frame_timing.record(
            "iced.create_window.surface",
            surface_start.elapsed(),
            1,
            timing_physical_size,
        );
        let renderer_start = Instant::now();
        let renderer = compositor.create_renderer();
        self.frame_timing.record(
            "iced.create_window.renderer",
            renderer_start.elapsed(),
            1,
            timing_physical_size,
        );
        let theme = iced::Theme::default(iced::theme::Mode::Light);
        let base = theme.base();
        let clipboard_start = Instant::now();
        let clipboard = iced_winit::Clipboard::connect(window.clone());
        self.frame_timing.record(
            "iced.create_window.clipboard",
            clipboard_start.elapsed(),
            1,
            timing_physical_size,
        );

        self.window = Some(NativeIcedWindow {
            raw: window.clone(),
            compositor,
            surface,
            surface_physical_size: physical_size,
            renderer,
            cache: user_interface::Cache::new(),
            clipboard,
            viewport: viewport_from_window(&physical_size, scale_factor),
            cursor_position: None,
            modifiers: ModifiersState::default(),
            mouse_interaction: mouse::Interaction::None,
            ime_state: None,
            preedit: None,
            preedit_text_color: None,
            preedit_font: None,
            preedit_text_size: None,
            ime_composing: false,
            ime_allowed,
            ime_policy: self.ime_policy,
            ime_trace: self.ime_trace,
            redraw_at: None,
            presented_frames: 0,
            pressed_mouse_buttons: 0,
            theme,
            style: renderer::Style {
                text_color: base.text_color,
            },
        });
        self.sync_presented_viewport_from_window();
        self.start_wake_thread();
        if let Some(window) = self.window.as_ref() {
            window.raw.request_redraw();
        }
        let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
        self.frame_timing.record(
            "iced.create_window",
            create_window_start.elapsed(),
            1,
            timing_viewport,
        );

        Ok(())
    }

    fn sync_presented_viewport_from_window(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let logical = window.viewport.logical_size();
        self.app.presented_viewport.set(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: slipway_core::Size {
                width: logical.width,
                height: logical.height,
            },
        });
        self.app.sync_presented_viewport();
    }

    fn cursor(&self) -> mouse::Cursor {
        let Some(window) = self.window.as_ref() else {
            return mouse::Cursor::Unavailable;
        };
        window
            .cursor_position
            .map(|position| {
                iced_winit::conversion::cursor_position(position, window.viewport.scale_factor())
            })
            .map(mouse::Cursor::Available)
            .unwrap_or(mouse::Cursor::Unavailable)
    }

    fn dispatch_iced_events(&mut self, events: &[iced::Event]) -> Vec<SlipwayIcedRuntimeMessage> {
        let timing_start = Instant::now();
        let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
        let cursor = self.cursor();
        let Some(window) = self.window.as_mut() else {
            return Vec::new();
        };

        let cache = std::mem::replace(&mut window.cache, user_interface::Cache::new());
        let mut messages = Vec::new();
        let mut user_interface = UserInterface::build(
            self.app.view::<iced::Theme, iced::Renderer>(),
            window.viewport.logical_size(),
            cache,
            &mut window.renderer,
        );
        let (state, _statuses) = user_interface.update(
            events,
            cursor,
            &mut window.renderer,
            &mut window.clipboard,
            &mut messages,
        );
        trace_iced_events_for_ime(window.ime_trace, events);
        let (mouse_interaction, input_method, redraw_request) = match state {
            user_interface::State::Updated {
                mouse_interaction,
                input_method,
                redraw_request,
                ..
            } => (
                Some(mouse_interaction),
                Some(input_method),
                Some(redraw_request),
            ),
            user_interface::State::Outdated => (None, None, None),
        };
        window.cache = user_interface.into_cache();
        self.apply_preedit_style_messages(&messages);
        if let Some(input_method) = input_method {
            let Some(window) = self.window.as_mut() else {
                return messages;
            };
            request_window_input_method(window, input_method);
        }
        if let Some(redraw_request) = redraw_request {
            let Some(window) = self.window.as_mut() else {
                return messages;
            };
            request_window_redraw(window, redraw_request);
        }
        if let Some(mouse_interaction) = mouse_interaction {
            self.update_mouse_interaction(mouse_interaction);
        }
        self.record_iced_update_message_counts(&messages, timing_viewport);
        self.frame_timing.record(
            "iced.update",
            timing_start.elapsed(),
            events.len(),
            timing_viewport,
        );
        messages
    }

    fn dispatch_iced_events_with_commit_mutation_gate(
        &mut self,
        events: &[iced::Event],
        gate: &IcedNativeCommitMutationGate,
    ) -> Vec<SlipwayIcedRuntimeMessage> {
        let timing_start = Instant::now();
        let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
        let cursor = self.cursor();
        let Some(window) = self.window.as_mut() else {
            return Vec::new();
        };

        let cache = std::mem::replace(&mut window.cache, user_interface::Cache::new());
        let mut messages = Vec::new();
        let mut user_interface = UserInterface::build(
            self.app
                .view_with_iced_native_commit_mutation_gate::<iced::Theme, iced::Renderer>(gate),
            window.viewport.logical_size(),
            cache,
            &mut window.renderer,
        );
        let (state, _statuses) = user_interface.update(
            events,
            cursor,
            &mut window.renderer,
            &mut window.clipboard,
            &mut messages,
        );
        trace_iced_events_for_ime(window.ime_trace, events);
        let (mouse_interaction, input_method, redraw_request) = match state {
            user_interface::State::Updated {
                mouse_interaction,
                input_method,
                redraw_request,
                ..
            } => (
                Some(mouse_interaction),
                Some(input_method),
                Some(redraw_request),
            ),
            user_interface::State::Outdated => (None, None, None),
        };
        window.cache = user_interface.into_cache();
        self.apply_preedit_style_messages(&messages);
        if let Some(input_method) = input_method
            && let Some(window) = self.window.as_mut()
        {
            request_window_input_method(window, input_method);
        }
        if let Some(redraw_request) = redraw_request
            && let Some(window) = self.window.as_mut()
        {
            request_window_redraw(window, redraw_request);
        }
        if let Some(mouse_interaction) = mouse_interaction {
            self.update_mouse_interaction(mouse_interaction);
        }
        self.record_iced_update_message_counts(&messages, timing_viewport);
        self.frame_timing.record(
            "iced.update",
            timing_start.elapsed(),
            events.len(),
            timing_viewport,
        );
        messages
    }

    fn process_messages(&mut self, messages: Vec<SlipwayIcedRuntimeMessage>) {
        let mut should_redraw = false;
        for message in messages {
            if let SlipwayIcedRuntimeMessage::PreeditStyle(style) = message {
                self.apply_preedit_style(style);
                should_redraw = true;
                continue;
            }
            let update = self.app.update_without_debug_drain(message);
            should_redraw |= self.update_requires_redraw(&update);
        }
        if should_redraw {
            if let Some(window) = self.window.as_ref() {
                window.raw.request_redraw();
            }
        }
    }

    fn process_messages_for_ingress(
        &mut self,
        origin: &IcedIngressOrigin,
        gate: Option<&IcedNativeCommitMutationGate>,
        messages: Vec<SlipwayIcedRuntimeMessage>,
    ) {
        if matches!(origin, IcedIngressOrigin::NativeOs) {
            let messages = messages
                .into_iter()
                .map(|message| retag_iced_ingress_message(message, "physical-input/native-os"))
                .collect();
            self.process_messages(messages);
            return;
        }

        let mut should_redraw = false;
        for message in messages {
            match message {
                SlipwayIcedRuntimeMessage::PreeditStyle(style) => {
                    self.apply_preedit_style(style);
                    should_redraw = true;
                }
                SlipwayIcedRuntimeMessage::DispatchRefusal(evidence) => {
                    if let Some(pending) = self.pending_physical_control.as_mut() {
                        if let PendingIcedPhysicalEvidence::Ordinary { last_refusal, .. } =
                            &mut pending.evidence
                        {
                            *last_refusal = Some(evidence.clone());
                        }
                    }
                    self.app
                        .runtime_mut()
                        .record_dispatch_refusal_for_backend(evidence, ICED_BACKEND_ID);
                }
                SlipwayIcedRuntimeMessage::BackendInput(mut input) => {
                    let pass = if matches!(origin, IcedIngressOrigin::DebugComposition { .. }) {
                        DEBUG_COMPOSITION_PASS_ID
                    } else {
                        "physical-input/debug-injected"
                    };
                    retag_backend_input(&mut input, pass);
                    let ordinary_match =
                        self.pending_physical_control
                            .as_ref()
                            .is_some_and(|pending| {
                                pending.token == ingress_token(origin)
                                    && matches!(
                                        pending.evidence,
                                        PendingIcedPhysicalEvidence::Ordinary { .. }
                                    )
                                    && self
                                        .app
                                        .runtime()
                                        .backend_presented_physical_control_input_matches(
                                            &pending.command,
                                            &input,
                                        )
                            });
                    let composition_event = match &input.event {
                        InputEvent::TextComposition(event) => Some(event.clone()),
                        _ => None,
                    };
                    let update = self
                        .app
                        .update_without_debug_drain(SlipwayIcedRuntimeMessage::BackendInput(input));
                    should_redraw |= self.update_requires_redraw(&update);
                    let trace = self.app.runtime().last_backend_input_trace().cloned();
                    if let Some(trace) = trace {
                        if ordinary_match {
                            if let Some(PendingIcedNativePhysicalControl {
                                evidence:
                                    PendingIcedPhysicalEvidence::Ordinary { matched_trace, .. },
                                ..
                            }) = self.pending_physical_control.as_mut()
                            {
                                *matched_trace = Some(trace.clone());
                            }
                        }
                        if let (
                            IcedIngressOrigin::DebugComposition {
                                token,
                                sequence_index,
                                phase,
                            },
                            Some(event),
                        ) = (origin, composition_event)
                        {
                            self.record_composition_phase_trace(
                                *token,
                                *sequence_index,
                                *phase,
                                event,
                                trace,
                            );
                        }
                    }
                }
                SlipwayIcedRuntimeMessage::NativeTextMutation(mut mutation) => {
                    retag_backend_input(&mut mutation.input, DEBUG_COMPOSITION_PASS_ID);
                    let gate_matches = matches!(
                        (origin, gate),
                        (
                            IcedIngressOrigin::DebugComposition {
                                token,
                                sequence_index,
                                phase: TextCompositionPhase::Commit,
                            },
                            Some(gate),
                        ) if *token == gate.token
                            && *sequence_index == gate.commit_sequence_index
                            && mutation.target == gate.target
                            && mutation.target_slot == gate.target_slot
                            && mutation.region_id == gate.region_id
                    );
                    if !gate_matches {
                        continue;
                    }
                    let input = mutation.input;
                    let update = self
                        .app
                        .update_without_debug_drain(SlipwayIcedRuntimeMessage::BackendInput(input));
                    should_redraw |= self.update_requires_redraw(&update);
                    if let Some(trace) = self.app.runtime().last_backend_input_trace().cloned()
                        && let IcedIngressOrigin::DebugComposition {
                            token,
                            sequence_index,
                            phase,
                        } = origin
                    {
                        self.record_composition_commit_mutation(
                            *token,
                            *sequence_index,
                            *phase,
                            mutation.before,
                            mutation.after,
                            trace,
                        );
                    }
                }
                other => {
                    let update = self.app.update_without_debug_drain(other);
                    should_redraw |= self.update_requires_redraw(&update);
                }
            }
        }
        if should_redraw && let Some(window) = self.window.as_ref() {
            window.raw.request_redraw();
        }

        match origin {
            IcedIngressOrigin::Debug {
                token,
                final_in_plan: true,
            } => self.finish_ordinary_physical_control(*token),
            IcedIngressOrigin::DebugComposition {
                token,
                phase: TextCompositionPhase::End,
                ..
            } => self.finish_composition_control(*token),
            _ => {}
        }
    }

    fn record_composition_phase_trace(
        &mut self,
        token: u64,
        sequence_index: usize,
        phase: TextCompositionPhase,
        event: TextCompositionEvent,
        trace: BackendInputTrace,
    ) {
        let Some(pending) = self.pending_physical_control.as_mut() else {
            return;
        };
        if pending.token != token {
            return;
        }
        let PendingIcedPhysicalEvidence::Composition { phases, .. } = &mut pending.evidence else {
            return;
        };
        let Some(dispatch_evidence) = trace.input.dispatch_evidence.clone() else {
            return;
        };
        phases.push(DebugCompositionPhaseTrace {
            phase,
            backend_event: composition_backend_event_label(phase).to_string(),
            provenance: CompositionPhaseProvenance::Native,
            event,
            ingress_observation: DebugCompositionIngressObservation::IcedQueueSlice {
                sequence_index,
            },
            dispatch_evidence,
            app_handled: trace.handled,
            result_identity: Some(trace.event_probe().result_identity),
        });
    }

    fn record_composition_commit_mutation(
        &mut self,
        token: u64,
        sequence_index: usize,
        phase: TextCompositionPhase,
        before: String,
        after: String,
        trace: BackendInputTrace,
    ) {
        let Some(pending) = self.pending_physical_control.as_mut() else {
            return;
        };
        if pending.token != token {
            return;
        }
        let PendingIcedPhysicalEvidence::Composition {
            phases,
            commit_mutation,
            target,
            target_slot,
            region,
            ..
        } = &mut pending.evidence
        else {
            return;
        };
        let Some(event) = composition_event_for_command(
            &pending.command,
            sequence_index,
            target,
            target_slot.as_ref(),
        ) else {
            return;
        };
        let Some(mut dispatch_evidence) = trace.input.dispatch_evidence.clone() else {
            return;
        };
        dispatch_evidence.generated_event = Some(InputEvent::TextComposition(event.clone()));
        dispatch_evidence.selected_region = Some(region.clone());
        dispatch_evidence.source = slipway_core::EvidenceSource::backend_presented(
            ICED_BACKEND_ID,
            DEBUG_COMPOSITION_PASS_ID,
        );
        let result_identity = trace.event_probe().result_identity;
        phases.push(DebugCompositionPhaseTrace {
            phase,
            backend_event: composition_backend_event_label(phase).to_string(),
            provenance: CompositionPhaseProvenance::Native,
            event,
            ingress_observation: DebugCompositionIngressObservation::IcedQueueSlice {
                sequence_index,
            },
            dispatch_evidence: dispatch_evidence.clone(),
            app_handled: trace.handled,
            result_identity: Some(result_identity.clone()),
        });
        let control_trace = DebugControlTrace::new(
            pending.command.request_id.clone(),
            pending.command.frame_identity().clone(),
            &trace.input.event,
            trace.handled,
            trace.revision_before.unwrap_or(
                trace
                    .input
                    .dispatch_evidence
                    .as_ref()
                    .map_or(0, |e| e.frame.revision),
            ),
            trace
                .revision_after
                .unwrap_or(trace.revision_before.unwrap_or(0)),
            trace.diagnostics.clone(),
        )
        .with_mode(DebugControlMode::PhysicalEquivalent)
        .with_dispatch_evidence(Some(dispatch_evidence))
        .with_result_identity(result_identity);
        if trace.handled
            && trace.input.event.target() == target
            && trace.input.event.target_slot() == target_slot.as_ref()
            && before != after
        {
            *commit_mutation = Some(DebugCompositionCommitMutation {
                trace: control_trace,
                before,
                after,
            });
        }
    }

    fn finish_ordinary_physical_control(&mut self, token: u64) {
        let Some(pending) = self.pending_physical_control.take() else {
            return;
        };
        if pending.token != token {
            self.pending_physical_control = Some(pending);
            return;
        }
        let PendingIcedPhysicalEvidence::Ordinary {
            matched_trace,
            last_refusal,
        } = pending.evidence
        else {
            self.pending_physical_control = Some(pending);
            return;
        };
        let product = matched_trace.map_or_else(
            || DebugReplyProduct::Error(native_physical_no_match_failure(last_refusal, 0)),
            |trace| {
                self.app
                    .runtime()
                    .backend_presented_physical_control_product_from_trace_for_backend(
                        pending.command.clone(),
                        &trace,
                        ICED_BACKEND_ID,
                    )
            },
        );
        let _ = pending.lease.complete(product);
        let _ = pending.pending.try_finish_and_respond();
    }

    fn finish_composition_control(&mut self, token: u64) {
        let Some(pending) = self.pending_physical_control.take() else {
            return;
        };
        if pending.token != token {
            self.pending_physical_control = Some(pending);
            return;
        }
        let PendingIcedPhysicalEvidence::Composition {
            target,
            target_slot: _,
            region,
            focused_before,
            phases,
            commit_mutation,
        } = pending.evidence
        else {
            self.pending_physical_control = Some(pending);
            return;
        };
        let focused_after = self
            .focused_visible_region()
            .is_some_and(|focused| focused == region);
        let trace = DebugCompositionTrace {
            request_id: pending.command.request_id.clone(),
            frame: pending.command.frame_identity().clone(),
            backend_id: ICED_BACKEND_ID.to_string(),
            target,
            selected_region: region,
            focused_before,
            focused_after,
            phases,
            commit_mutation,
            completed: true,
            failure: None,
        };
        let product = self
            .app
            .runtime()
            .backend_presented_text_composition_product_from_traces_for_backend(
                pending.command.clone(),
                trace,
                ICED_BACKEND_ID,
            );
        let _ = pending.lease.complete(product);
        let _ = pending.pending.try_finish_and_respond();
    }

    fn update_requires_redraw(&self, update: &SlipwayIcedRuntimeAppUpdate) -> bool {
        let trace_requires_redraw = self
            .app
            .runtime()
            .last_backend_input_trace()
            .is_some_and(|trace| !trace.changes.is_empty() || !trace.emitted_messages.is_empty());
        iced_runtime_update_requires_redraw(update, trace_requires_redraw)
    }

    fn apply_preedit_style_messages(&mut self, messages: &[SlipwayIcedRuntimeMessage]) {
        for message in messages {
            if let SlipwayIcedRuntimeMessage::PreeditStyle(style) = message {
                self.apply_preedit_style(style.clone());
            }
        }
    }

    fn apply_preedit_style(&mut self, style: IcedPreeditOverlayStyle) {
        let Some(window) = self.window.as_mut() else {
            return;
        };
        window.preedit_text_color = Some(style.text_color);
        window.preedit_font = Some(style.font);
        window.preedit_text_size = Some(style.size);
    }

    fn update_mouse_interaction(&mut self, interaction: mouse::Interaction) {
        let Some(window) = self.window.as_mut() else {
            return;
        };
        apply_window_mouse_interaction(window, interaction);
    }

    fn redraw(&mut self) -> RedrawFrameFlow {
        let timing_start = Instant::now();
        let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
        let cursor = self.cursor();
        let pending_capture = self.pending_presented_capture.take();
        let render_candidate = self.app.runtime().last_frame_identity().clone();
        let Some(window) = self.window.as_mut() else {
            self.pending_presented_capture = pending_capture;
            return RedrawFrameFlow::Continue;
        };
        let presented_frame_before = window.presented_frames;
        let cache = std::mem::replace(&mut window.cache, user_interface::Cache::new());
        let view_start = Instant::now();
        let element = self.app.view::<iced::Theme, iced::Renderer>();
        let view_elapsed = view_start.elapsed();
        let build_start = Instant::now();
        let mut user_interface = UserInterface::build(
            element,
            window.viewport.logical_size(),
            cache,
            &mut window.renderer,
        );
        let build_elapsed = build_start.elapsed();
        let draw_start = Instant::now();
        user_interface.draw(&mut window.renderer, &window.theme, &window.style, cursor);
        let draw_elapsed = draw_start.elapsed();
        window.cache = user_interface.into_cache();
        let has_preedit = window.preedit.is_some();
        let preedit_start = Instant::now();
        draw_window_preedit(window);
        let preedit_elapsed = preedit_start.elapsed();
        let base = window.theme.base();
        let raw = window.raw.clone();
        let present_start = Instant::now();
        let present_outcome = present_iced_window(
            window,
            render_candidate,
            &mut self.backend_present_index,
            &mut self.last_successfully_presented,
            pending_capture,
            base.background_color,
            || raw.pre_present_notify(),
        );
        let present_elapsed = present_start.elapsed();
        let present_recovery = match present_outcome.result {
            IcedPresentationResult::Presented => {
                window.presented_frames += 1;
                None
            }
            IcedPresentationResult::CompositorFailed(error) => {
                Some(recover_window_from_present_error(window, error))
            }
            IcedPresentationResult::SkippedOverflow => None,
        };
        let presented_frame_after = window.presented_frames;
        let _ = window;
        self.pending_presented_capture = present_outcome.pending_capture;
        if let Some((capture, code, reason)) = present_outcome.capture_failure {
            complete_presented_capture_refusal(
                capture.command,
                capture.lease,
                capture.pending,
                None,
                code,
                reason,
            );
            self.pump_mcp();
        }
        self.frame_timing.record(
            "iced.redraw.frame_index",
            Duration::ZERO,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.draw",
            draw_elapsed,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.preedit_draw",
            preedit_elapsed,
            usize::from(has_preedit),
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.present",
            present_elapsed,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.view",
            view_elapsed,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.build",
            build_elapsed,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.presented_frame_count",
            Duration::ZERO,
            presented_frame_after as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.draw_present",
            timing_start.elapsed(),
            presented_frame_before as usize,
            timing_viewport,
        );
        if present_recovery.is_some() {
            self.frame_timing
                .record("iced.present_error", Duration::ZERO, 1, timing_viewport);
        }
        if present_recovery == Some(PresentErrorRecovery::Exit) {
            RedrawFrameFlow::Exit
        } else {
            RedrawFrameFlow::Continue
        }
    }

    /// Runs one `WindowEvent::RedrawRequested` frame with iced 0.14's frame
    /// protocol: the synthetic `window::Event::RedrawRequested` tick shares
    /// one `UserInterface::build` with the draw, mirroring iced_winit's
    /// update-then-draw redraw handler. The pending-queue safety-net drain
    /// (Step 181) runs first through `dispatch_pending_iced_events`, one
    /// slice at a time with message application in between, so the tick's
    /// build never stamps dispatch evidence against a frame revision that an
    /// earlier queued event's message is about to bump (see
    /// `pending_dispatch_slices`); the tick then observes the post-apply
    /// state on a fresh build. Messages emitted by the tick update itself
    /// are rare; they take the existing message-application flow and accept
    /// the one extra rebuild that flow implies.
    fn redraw_requested_frame(&mut self) -> RedrawFrameFlow {
        self.dispatch_pending_iced_events();
        let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
        let cursor = self.cursor();
        let pending_capture = self.pending_presented_capture.take();
        let render_candidate = self.app.runtime().last_frame_identity().clone();
        let redraw_event =
            iced::Event::Window(iced::window::Event::RedrawRequested(Instant::now()));
        let events = [redraw_event.clone()];
        let event_count = events.len();
        let Some(window) = self.window.as_mut() else {
            self.pending_presented_capture = pending_capture;
            return RedrawFrameFlow::Continue;
        };
        let presented_frame_before = window.presented_frames;
        let cache = std::mem::replace(&mut window.cache, user_interface::Cache::new());
        let view_start = Instant::now();
        let element = self.app.view::<iced::Theme, iced::Renderer>();
        let view_elapsed = view_start.elapsed();
        let build_start = Instant::now();
        let mut user_interface = UserInterface::build(
            element,
            window.viewport.logical_size(),
            cache,
            &mut window.renderer,
        );
        let build_elapsed = build_start.elapsed();

        let update_start = Instant::now();
        let mut messages = Vec::new();
        let (mut state, _statuses) = user_interface.update(
            &events,
            cursor,
            &mut window.renderer,
            &mut window.clipboard,
            &mut messages,
        );
        let mut redraw_updates_delivered = 1;
        // iced_winit re-ticks the same interface when a redraw update
        // invalidated layout without messages, so relayout-armed widgets
        // (for example scrollable viewport notification) observe the fresh
        // layout in the same frame; the bound matches iced_winit's
        // three-update cap.
        while should_re_tick_redraw_update(
            messages.is_empty(),
            state.has_layout_changed(),
            redraw_updates_delivered,
        ) {
            let (next_state, _statuses) = user_interface.update(
                std::slice::from_ref(&redraw_event),
                cursor,
                &mut window.renderer,
                &mut window.clipboard,
                &mut messages,
            );
            state = next_state;
            redraw_updates_delivered += 1;
        }
        let update_elapsed = update_start.elapsed();
        trace_iced_events_for_ime(window.ime_trace, &events);
        let (mouse_interaction, input_method, redraw_request) = match state {
            user_interface::State::Updated {
                mouse_interaction,
                input_method,
                redraw_request,
                ..
            } => (
                Some(mouse_interaction),
                Some(input_method),
                Some(redraw_request),
            ),
            user_interface::State::Outdated => (None, None, None),
        };

        if !messages.is_empty() {
            window.cache = user_interface.into_cache();
            let _ = window;
            self.pending_presented_capture = pending_capture;
            self.apply_preedit_style_messages(&messages);
            if let Some(input_method) = input_method {
                if let Some(window) = self.window.as_mut() {
                    request_window_input_method(window, input_method);
                }
            }
            if let Some(redraw_request) = redraw_request {
                if let Some(window) = self.window.as_mut() {
                    request_window_redraw(window, redraw_request);
                }
            }
            if let Some(mouse_interaction) = mouse_interaction {
                self.update_mouse_interaction(mouse_interaction);
            }
            self.record_iced_update_message_counts(&messages, timing_viewport);
            self.frame_timing
                .record("iced.update", update_elapsed, event_count, timing_viewport);
            self.process_messages(messages);
            // The accepted extra rebuild: the presented frame must reflect
            // the state the applied messages just produced.
            return self.redraw();
        }

        let draw_present_start = Instant::now();
        let draw_start = Instant::now();
        user_interface.draw(&mut window.renderer, &window.theme, &window.style, cursor);
        let draw_elapsed = draw_start.elapsed();
        window.cache = user_interface.into_cache();
        if let Some(input_method) = input_method {
            request_window_input_method(window, input_method);
        }
        if let Some(redraw_request) = redraw_request {
            request_window_redraw(window, redraw_request);
        }
        if let Some(mouse_interaction) = mouse_interaction {
            apply_window_mouse_interaction(window, mouse_interaction);
        }
        let has_preedit = window.preedit.is_some();
        let preedit_start = Instant::now();
        draw_window_preedit(window);
        let preedit_elapsed = preedit_start.elapsed();
        let base = window.theme.base();
        let raw = window.raw.clone();
        let present_start = Instant::now();
        let present_outcome = present_iced_window(
            window,
            render_candidate,
            &mut self.backend_present_index,
            &mut self.last_successfully_presented,
            pending_capture,
            base.background_color,
            || raw.pre_present_notify(),
        );
        let present_elapsed = present_start.elapsed();
        let present_recovery = match present_outcome.result {
            IcedPresentationResult::Presented => {
                window.presented_frames += 1;
                None
            }
            IcedPresentationResult::CompositorFailed(error) => {
                Some(recover_window_from_present_error(window, error))
            }
            IcedPresentationResult::SkippedOverflow => None,
        };
        let presented_frame_after = window.presented_frames;
        let _ = window;
        self.pending_presented_capture = present_outcome.pending_capture;
        if let Some((capture, code, reason)) = present_outcome.capture_failure {
            complete_presented_capture_refusal(
                capture.command,
                capture.lease,
                capture.pending,
                None,
                code,
                reason,
            );
            self.pump_mcp();
        }
        self.frame_timing
            .record("iced.update", update_elapsed, event_count, timing_viewport);
        self.frame_timing.record(
            "iced.redraw.frame_index",
            Duration::ZERO,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.draw",
            draw_elapsed,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.preedit_draw",
            preedit_elapsed,
            usize::from(has_preedit),
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.present",
            present_elapsed,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.view",
            view_elapsed,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.build",
            build_elapsed,
            presented_frame_before as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.presented_frame_count",
            Duration::ZERO,
            presented_frame_after as usize,
            timing_viewport,
        );
        self.frame_timing.record(
            "iced.draw_present",
            draw_present_start.elapsed(),
            presented_frame_before as usize,
            timing_viewport,
        );
        if present_recovery.is_some() {
            self.frame_timing
                .record("iced.present_error", Duration::ZERO, 1, timing_viewport);
        }
        if present_recovery == Some(PresentErrorRecovery::Exit) {
            RedrawFrameFlow::Exit
        } else {
            RedrawFrameFlow::Continue
        }
    }

    fn handle_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window_id) = self.window.as_ref().map(|window| window.raw.id()) else {
            return;
        };
        if id != window_id {
            return;
        }
        self.record_window_event_kind_for_timing(&event);

        match &event {
            WindowEvent::CloseRequested => {
                self.refuse_pending_presented_capture_on_teardown();
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                self.queue_resize(*size);
                return;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.queue_scale_factor_resize(*scale_factor as f32);
                return;
            }
            WindowEvent::Focused(true) => {
                if let Some(window) = self.window.as_mut() {
                    ensure_window_ime_policy(window, "focused");
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(window) = self.window.as_mut() {
                    trace_ime(
                        window.ime_trace,
                        format_args!(
                            "raw KeyboardInput logical={:?} physical={:?} text={:?} state={:?}",
                            event.logical_key, event.physical_key, event.text, event.state
                        ),
                    );
                    ensure_window_ime_policy(window, "keyboard-input");
                }
            }
            WindowEvent::Ime(event) => {
                if let Some(window) = self.window.as_mut() {
                    trace_ime(window.ime_trace, format_args!("raw Ime::{event:?}"));
                    match event {
                        Ime::Enabled => {
                            window.ime_composing = true;
                        }
                        Ime::Preedit(content, selection) => {
                            window.ime_composing = !content.is_empty() || selection.is_some();
                            if !window.ime_composing {
                                window.preedit = None;
                            }
                        }
                        Ime::Commit(_) | Ime::Disabled => {
                            window.ime_composing = false;
                            window.preedit = None;
                        }
                    }
                    ensure_window_ime_policy(window, "raw-ime-event");
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(window) = self.window.as_mut() {
                    window.cursor_position = Some(*position);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(window) = self.window.as_mut() {
                    window.cursor_position = None;
                }
            }
            WindowEvent::MouseInput { state, .. } => {
                if let Some(window) = self.window.as_mut() {
                    match state {
                        ElementState::Pressed => {
                            window.pressed_mouse_buttons =
                                window.pressed_mouse_buttons.saturating_add(1);
                        }
                        ElementState::Released => {
                            window.pressed_mouse_buttons =
                                window.pressed_mouse_buttons.saturating_sub(1);
                        }
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                if let Some(window) = self.window.as_mut() {
                    window.modifiers = modifiers.state();
                }
            }
            WindowEvent::RedrawRequested => {
                let timing_start = Instant::now();
                let pending_event_count = self.pending_iced_events.len();
                let should_defer_resize = self.pending_resize.is_some()
                    && self
                        .window
                        .as_ref()
                        .is_some_and(|window| window.presented_frames > 0);
                if should_defer_resize {
                    let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
                    self.frame_timing.record(
                        "iced.resize_deferred_redraw",
                        timing_start.elapsed(),
                        pending_event_count,
                        timing_viewport,
                    );
                    return;
                }
                self.apply_pending_resize(false);
                // iced 0.14 frame protocol: every RedrawRequested delivers
                // the synthetic redraw event to the widget tree; the pending
                // safety-net drain (Step 181) merges into the same single
                // UserInterface build/update as the draw.
                let flow = self.redraw_requested_frame();
                let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
                self.frame_timing.record(
                    "iced.redraw_requested",
                    timing_start.elapsed(),
                    pending_event_count,
                    timing_viewport,
                );
                if flow == RedrawFrameFlow::Exit {
                    event_loop.exit();
                }
                return;
            }
            _ => {}
        }

        let Some((scale_factor, modifiers)) = self
            .window
            .as_ref()
            .map(|window| (window.viewport.scale_factor(), window.modifiers))
        else {
            return;
        };
        if let Some(event) = iced_winit::conversion::window_event(event, scale_factor, modifiers) {
            let is_cursor_moved = is_iced_cursor_moved_event(&event);
            let is_wheel_scrolled = is_iced_wheel_event(&event);
            self.queue_pending_iced_event(event);
            if self.pending_resize.is_none() {
                if is_cursor_moved {
                    if let Some(window) = self.window.as_ref() {
                        window.raw.request_redraw();
                    }
                } else if is_wheel_scrolled {
                    // Wheel ticks stay queued so a same-iteration OS burst can
                    // coalesce into one pending event. `about_to_wait` runs
                    // after the OS event batch and dispatches the queue this
                    // iteration, then requests the redraw, so wheel latency
                    // stays within the same event-loop iteration.
                } else {
                    self.dispatch_pending_iced_events();
                    if let Some(window) = self.window.as_ref() {
                        window.raw.request_redraw();
                    }
                }
            } else if self
                .window
                .as_ref()
                .is_some_and(|window| window.presented_frames == 0)
            {
                if let Some(window) = self.window.as_ref() {
                    window.raw.request_redraw();
                }
            }
        }
    }

    fn record_window_event_kind_for_timing(&mut self, event: &WindowEvent) {
        let kind = match event {
            WindowEvent::CursorMoved { .. } => "iced.raw.cursor_moved",
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => "iced.raw.mouse_pressed",
            WindowEvent::MouseInput {
                state: ElementState::Released,
                ..
            } => "iced.raw.mouse_released",
            WindowEvent::MouseWheel { .. } => "iced.raw.mouse_wheel",
            WindowEvent::RedrawRequested => "iced.raw.redraw_requested",
            WindowEvent::Resized(_) => "iced.raw.resized",
            WindowEvent::ScaleFactorChanged { .. } => "iced.raw.scale_factor_changed",
            WindowEvent::Focused(true) => "iced.raw.focused",
            WindowEvent::Focused(false) => "iced.raw.unfocused",
            WindowEvent::KeyboardInput { .. } => "iced.raw.keyboard_input",
            WindowEvent::Ime(_) => "iced.raw.ime",
            _ => return,
        };
        let viewport = self.window.as_ref().map(iced_timing_viewport);
        self.frame_timing.record(kind, Duration::ZERO, 0, viewport);
    }

    fn record_iced_update_message_counts(
        &mut self,
        messages: &[SlipwayIcedRuntimeMessage],
        viewport: Option<Size>,
    ) {
        let backend_inputs = messages
            .iter()
            .filter(|message| matches!(message, SlipwayIcedRuntimeMessage::BackendInput(_)))
            .count();
        let preedit_messages = messages
            .iter()
            .filter(|message| matches!(message, SlipwayIcedRuntimeMessage::PreeditStyle(_)))
            .count();
        self.frame_timing.record(
            "iced.update.backend_input_messages",
            Duration::ZERO,
            backend_inputs,
            viewport,
        );
        self.frame_timing.record(
            "iced.update.preedit_messages",
            Duration::ZERO,
            preedit_messages,
            viewport,
        );
    }

    fn queue_resize(&mut self, size: PhysicalSize<u32>) {
        let Some(window) = self.window.as_mut() else {
            return;
        };
        let physical_size = non_zero_physical_size(size);
        let request_initial_redraw = window.presented_frames == 0;
        window.viewport = Viewport::with_physical_size(
            iced::Size::new(physical_size.width, physical_size.height),
            window.raw.scale_factor() as f32,
        );
        let raw = window.raw.clone();
        let unchanged_surface = window.surface_physical_size == physical_size;
        self.pending_resize = (!unchanged_surface).then_some(PendingResize {
            physical_size,
            queued_at: Instant::now(),
        });
        let _ = window;
        self.sync_presented_viewport_from_window();
        if request_initial_redraw || unchanged_surface {
            raw.request_redraw();
        }
    }

    fn queue_scale_factor_resize(&mut self, scale_factor: f32) {
        let Some(window) = self.window.as_mut() else {
            return;
        };
        let physical_size = non_zero_physical_size(window.raw.inner_size());
        let request_initial_redraw = window.presented_frames == 0;
        window.viewport = Viewport::with_physical_size(
            iced::Size::new(physical_size.width, physical_size.height),
            scale_factor,
        );
        let raw = window.raw.clone();
        let unchanged_surface = window.surface_physical_size == physical_size;
        self.pending_resize = (!unchanged_surface).then_some(PendingResize {
            physical_size,
            queued_at: Instant::now(),
        });
        let _ = window;
        self.sync_presented_viewport_from_window();
        if request_initial_redraw || unchanged_surface {
            raw.request_redraw();
        }
    }

    fn pending_resize_due_at(&self) -> Option<Instant> {
        self.pending_resize
            .as_ref()
            .map(|pending| pending.queued_at + RESIZE_CONFIGURE_QUIET)
    }

    fn apply_pending_resize(&mut self, request_redraw_after_configure: bool) {
        let Some(pending) = self.pending_resize.take() else {
            return;
        };
        let physical_size = pending.physical_size;
        let timing_start = Instant::now();
        let Some(window) = self.window.as_mut() else {
            return;
        };
        window.compositor.configure_surface(
            &mut window.surface,
            physical_size.width,
            physical_size.height,
        );
        window.surface_physical_size = physical_size;
        let raw = window.raw.clone();
        let _ = window;
        self.sync_presented_viewport_from_window();
        if request_redraw_after_configure {
            raw.request_redraw();
        }
        let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
        self.queue_latest_iced_resize_event();
        self.frame_timing.record(
            "iced.resize_configure",
            timing_start.elapsed(),
            0,
            timing_viewport,
        );
    }

    fn queue_latest_iced_resize_event(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let logical = window.viewport.logical_size();
        self.pending_iced_events.push(QueuedIcedIngress {
            event: iced::Event::Window(iced::window::Event::Resized(iced::Size::new(
                logical.width,
                logical.height,
            ))),
            origin: IcedIngressOrigin::NativeOs,
            commit_mutation_gate: None,
        });
    }

    fn queue_pending_iced_event(&mut self, event: iced::Event) {
        let is_wheel_scrolled = is_iced_wheel_event(&event);
        let coalesced = push_coalesced_pending_iced_event(
            &mut self.pending_iced_events,
            QueuedIcedIngress {
                event,
                origin: IcedIngressOrigin::NativeOs,
                commit_mutation_gate: None,
            },
        );
        if coalesced {
            let viewport = self.window.as_ref().map(iced_timing_viewport);
            let kind = if is_wheel_scrolled {
                "iced.wheel_scrolled_coalesced"
            } else {
                "iced.cursor_moved_coalesced"
            };
            self.frame_timing.record(kind, Duration::ZERO, 1, viewport);
        }
    }

    fn dispatch_pending_iced_events(&mut self) {
        if self.pending_iced_events.is_empty() {
            return;
        }
        let events = std::mem::take(&mut self.pending_iced_events);
        // Sequencing contract: every runtime message carries dispatch
        // evidence stamped with the frame revision of the
        // `UserInterface::build` that produced it, and a handled message
        // bumps that revision when applied. Flushing the whole queue through
        // one build/update let a queued wheel share a slice with a later
        // press or captured drag move; applying the wheel's message first
        // made the pointer message's evidence one revision stale, so the
        // runtime refused it (BACKEND_INPUT_DISPATCH_EVIDENCE_FRAME_MISMATCH)
        // and the grab/click was silently dropped until the next full
        // rebuild. Dispatching slice by slice — applying each slice's
        // messages before the next slice is built — restores the baseline
        // per-cycle ordering. Adjacent coalescing (Step 181) already
        // collapses cursor and wheel bursts, so the common queue holds one
        // event and still costs exactly one build. The debug-MCP physical
        // injection path keeps its own per-event dispatch in
        // `dispatch_native_physical_events` for evidence fidelity.
        for slice in pending_dispatch_slices(events) {
            let QueuedIcedDispatchSlice {
                events,
                origin,
                commit_mutation_gate,
            } = slice;
            let messages = match commit_mutation_gate.as_ref() {
                Some(gate) => self.dispatch_iced_events_with_commit_mutation_gate(&events, gate),
                None => self.dispatch_iced_events(&events),
            };
            self.process_messages_for_ingress(&origin, commit_mutation_gate.as_ref(), messages);
        }
    }

    fn pump_mcp(&mut self) {
        if self.pending_physical_control.is_some() || self.pending_presented_capture.is_some() {
            return;
        }
        loop {
            if self.pending_mcp.is_some() {
                self.complete_next_debug_lease_from_mcp();
                if self.pending_mcp.is_some() {
                    break;
                }
                continue;
            }
            let pending = match self.app.runtime_mut().take_pending_native_mcp_call() {
                Ok(Some(pending)) => pending,
                Ok(None) => break,
                Err(_) => break,
            };
            self.pending_mcp = Some(pending);
            self.complete_next_debug_lease_from_mcp();
            if self.pending_mcp.is_some() {
                break;
            }
        }
    }

    fn complete_next_debug_lease_from_mcp(&mut self) {
        let Some(pending) = self.pending_mcp.take() else {
            return;
        };
        self.app
            .runtime_mut()
            .record_presenting_backend(ICED_BACKEND_ID);
        let lease = match self.app.runtime_mut().take_debug_command_lease() {
            Ok(Some(lease)) => lease,
            Ok(None) | Err(_) => {
                self.pending_mcp = Some(pending);
                return;
            }
        };
        let command = lease.command().clone();
        if matches!(command.kind, DebugCommandKind::Screenshot { .. }) {
            self.arm_presented_capture(command, lease, pending);
            return;
        }
        let operation = match &command.kind {
            DebugCommandKind::PhysicalControl { operation, .. } => operation.clone(),
            _ => {
                let app = &mut self.app;
                let _ = app
                    .assembled
                    .runtime
                    .complete_debug_command_lease_with_app_reducer(
                        lease,
                        &mut app.apply_app_messages,
                    );
                let _ = pending.try_finish_and_respond();
                let _ = self.frame_timing.flush_to_file();
                return;
            }
        };
        match &operation {
            DebugPhysicalControl::Focus { .. }
            | DebugPhysicalControl::Command { .. }
            | DebugPhysicalControl::Scroll { .. }
            | DebugPhysicalControl::TextEdit {
                kind: slipway_core::TextEditKind::MoveCaret,
                ..
            } => {
                let product = self.run_native_physical_control(&command, &operation);
                self.refresh_visible_ui_cache_after_physical_control();
                let _ = lease.complete(product);
                let _ = pending.try_finish_and_respond();
            }
            DebugPhysicalControl::TextComposition { .. } => {
                if let Err((product, lease, pending)) =
                    self.queue_native_text_composition(command.clone(), &operation, lease, pending)
                {
                    let _ = lease.complete(product);
                    let _ = pending.try_finish_and_respond();
                }
            }
            _ => self.queue_native_event_physical_control(command, &operation, lease, pending),
        }
        let _ = self.frame_timing.flush_to_file();
    }

    fn next_request_token(&mut self) -> u64 {
        let token = self.next_debug_token;
        self.next_debug_token = self.next_debug_token.wrapping_add(1).max(1);
        token
    }

    fn arm_presented_capture(
        &mut self,
        command: DebugCommand,
        lease: DebugCommandLease,
        pending: SlipwayRuntimePendingNativeMcpCall,
    ) {
        self.arm_presented_capture_with_deadline_spawner(
            command,
            lease,
            pending,
            spawn_iced_capture_deadline,
        );
    }

    fn arm_presented_capture_with_deadline_spawner<S>(
        &mut self,
        command: DebugCommand,
        lease: DebugCommandLease,
        pending: SlipwayRuntimePendingNativeMcpCall,
        spawn_deadline: S,
    ) where
        S: FnOnce(
            EventLoopProxy<NativeRunnerEvent>,
            u64,
            Duration,
        ) -> std::io::Result<std::thread::JoinHandle<()>>,
    {
        if self.window.is_none() {
            complete_presented_capture_refusal(
                command,
                lease,
                pending,
                None,
                "screenshot-window-unavailable",
                "the iced visible window is not available for direct presented capture",
            );
            return;
        }
        match screenshot_selector(&command) {
            PresentedScreenshotSelector::Exact { expected_frame }
                if self.last_successfully_presented.as_ref() != Some(expected_frame) =>
            {
                complete_presented_capture_refusal(
                    command,
                    lease,
                    pending,
                    self.last_successfully_presented.clone(),
                    "screenshot-frame-mismatch",
                    "the requested frame is not the iced window's last successfully presented frame",
                );
                return;
            }
            PresentedScreenshotSelector::Exact { .. }
            | PresentedScreenshotSelector::Current { .. } => {}
        }
        let token = self.next_request_token();
        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(3);
        let wake: Arc<dyn iced_renderer::wgpu::window::compositor::DirectCaptureWake> =
            Arc::new(DirectCaptureWakeProxy {
                proxy: self.proxy.clone(),
            });
        let delay = Duration::from_secs(5);
        let deadline_proxy = self.proxy.clone();
        let capture = match build_iced_capture_lease(
            token,
            command,
            lease,
            pending,
            event_rx,
            event_tx,
            wake,
            delay,
            |delay| spawn_deadline(deadline_proxy, token, delay),
        ) {
            Ok(capture) => capture,
            Err((command, lease, pending)) => {
                complete_presented_capture_refusal(
                    command,
                    lease,
                    pending,
                    None,
                    "screenshot-deadline-thread-failed",
                    "the iced direct capture deadline thread could not be created",
                );
                return;
            }
        };
        self.pending_presented_capture = Some(PendingPresentedCapture::Armed(capture));
        if let Some(window) = self.window.as_ref() {
            window.raw.request_redraw();
        }
    }

    fn refuse_pending_presented_capture_on_teardown(&mut self) {
        let Some(capture) = self.pending_presented_capture.take() else {
            return;
        };
        let (capture, captured_frame) = match capture {
            PendingPresentedCapture::Armed(capture) => (capture, None),
            PendingPresentedCapture::Waiting {
                lease,
                committed_frame,
                ..
            } => {
                let (captured_frame, _, _) = iced_capture_teardown_refusal(Some(&committed_frame));
                (lease, captured_frame)
            }
        };
        complete_presented_capture_refusal(
            capture.command,
            capture.lease,
            capture.pending,
            captured_frame,
            "screenshot-window-teardown",
            "the iced visible window closed before direct presented capture completed",
        );
    }

    fn handle_direct_capture_wake(&mut self, token: u64) {
        let Some(state) = self.pending_presented_capture.take() else {
            return;
        };
        let state_token = match &state {
            PendingPresentedCapture::Armed(lease)
            | PendingPresentedCapture::Waiting { lease, .. } => lease.token,
        };
        if state_token != token {
            self.pending_presented_capture = Some(state);
            return;
        }
        let (lease, committed_frame, mut evidence) = match state {
            PendingPresentedCapture::Armed(lease) => (lease, None, IcedCaptureEvidence::default()),
            PendingPresentedCapture::Waiting {
                lease,
                committed_frame,
                evidence,
            } => (lease, Some(committed_frame), evidence),
        };
        let mut refusal = None;
        while let Ok(event) = lease.event_rx.try_recv() {
            match reduce_iced_capture_event(token, committed_frame.as_ref(), &mut evidence, event) {
                IcedCaptureReduction::Continue => {}
                IcedCaptureReduction::Refused { code, reason } => {
                    refusal = Some((code, reason));
                    break;
                }
            }
        }
        if refusal.is_none() && Instant::now() >= lease.deadline {
            let (_, code, reason) = iced_capture_deadline_refusal(committed_frame.as_ref());
            refusal = Some((code, reason));
        }
        if let Some((code, reason)) = refusal {
            let captured = committed_frame.clone();
            complete_presented_capture_refusal(
                lease.command,
                lease.lease,
                lease.pending,
                captured,
                code,
                reason,
            );
            self.pump_mcp();
            return;
        }
        if let (Some(meta), Some(bytes)) = (evidence.presented.as_ref(), evidence.mapped.as_ref()) {
            let captured_frame = committed_frame
                .as_ref()
                .expect("waiting direct capture owns its committed presentation");
            let selector = screenshot_selector(&lease.command);
            if let PresentedScreenshotSelector::Exact { expected_frame } = selector
                && !iced_capture_frame_is_exact_next(expected_frame, captured_frame)
            {
                complete_presented_capture_refusal(
                    lease.command,
                    lease.lease,
                    lease.pending,
                    Some(captured_frame.clone()),
                    "screenshot-captured-frame-mismatch",
                    "the iced direct capture did not present the exact next unchanged surface frame",
                );
                self.pump_mcp();
                return;
            }
            let expected_len = usize::try_from(meta.width)
                .ok()
                .and_then(|width| width.checked_mul(4))
                .and_then(|row| {
                    usize::try_from(meta.height)
                        .ok()
                        .and_then(|height| row.checked_mul(height))
                });
            if expected_len != Some(bytes.len()) {
                complete_presented_capture_refusal(
                    lease.command,
                    lease.lease,
                    lease.pending,
                    Some(captured_frame.clone()),
                    "screenshot-invalid-rgba-length",
                    "the iced direct readback did not contain tight width * height * 4 RGBA bytes",
                );
                self.pump_mcp();
                return;
            }
            let product = DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Captured(
                PresentedPixels {
                    selector: selector.clone(),
                    captured_frame: captured_frame.clone(),
                    source: slipway_core::EvidenceSource::backend_presented(
                        ICED_BACKEND_ID,
                        slipway_debug_bridge::PRESENTED_PIXELS_PASS_ID,
                    ),
                    capture_path: PresentedCapturePath::DirectAcquiredSurfaceTextureCopy,
                    source_format: presented_surface_format(meta.format),
                    transfer: presented_transfer(meta.format),
                    alpha: presented_alpha(meta.alpha),
                    width: meta.width,
                    height: meta.height,
                    bytes: Arc::clone(bytes),
                    diagnostics: Vec::new(),
                },
            ));
            let _ = lease.lease.complete(product);
            let _ = lease.pending.try_finish_and_respond();
            self.pump_mcp();
            return;
        }
        self.pending_presented_capture = Some(match committed_frame {
            Some(committed_frame) => PendingPresentedCapture::Waiting {
                lease,
                committed_frame,
                evidence,
            },
            None => PendingPresentedCapture::Armed(lease),
        });
    }

    fn queue_native_event_physical_control(
        &mut self,
        command: DebugCommand,
        operation: &DebugPhysicalControl,
        lease: DebugCommandLease,
        pending: SlipwayRuntimePendingNativeMcpCall,
    ) {
        if let DebugPhysicalControl::Keyboard { selector, .. } = operation
            && let Err(unsupported) = self.focus_native_region_for_selector(selector)
        {
            let _ = lease.complete(native_physical_control_error(unsupported));
            let _ = pending.try_finish_and_respond();
            return;
        }
        let events = match self.physical_control_events(operation) {
            Ok(events) if !events.is_empty() => events,
            Ok(_) => {
                let _ = lease.complete(DebugReplyProduct::Error(DebugFailure {
                    code: "native-physical-control-empty-event-plan".to_string(),
                    message:
                        "the iced native runner produced no native events for this physical control"
                            .to_string(),
                    dispatch_evidence: None,
                }));
                let _ = pending.try_finish_and_respond();
                return;
            }
            Err(unsupported) => {
                let _ = lease.complete(native_physical_control_error(unsupported));
                let _ = pending.try_finish_and_respond();
                return;
            }
        };
        let token = self.next_request_token();
        let event_count = events.len();
        self.pending_physical_control = Some(PendingIcedNativePhysicalControl {
            token,
            command,
            lease,
            pending,
            evidence: PendingIcedPhysicalEvidence::Ordinary {
                matched_trace: None,
                last_refusal: None,
            },
        });
        self.pending_iced_events.reserve(event_count);
        for (index, event) in events.into_iter().enumerate() {
            let queued = QueuedIcedIngress {
                event,
                origin: IcedIngressOrigin::Debug {
                    token,
                    final_in_plan: index + 1 == event_count,
                },
                commit_mutation_gate: None,
            };
            push_coalesced_pending_iced_event(&mut self.pending_iced_events, queued);
        }
        if let Some(window) = self.window.as_ref() {
            window.raw.request_redraw();
        }
    }

    fn queue_native_text_composition(
        &mut self,
        command: DebugCommand,
        operation: &DebugPhysicalControl,
        lease: DebugCommandLease,
        pending: SlipwayRuntimePendingNativeMcpCall,
    ) -> Result<
        (),
        (
            DebugReplyProduct,
            DebugCommandLease,
            SlipwayRuntimePendingNativeMcpCall,
        ),
    > {
        let DebugPhysicalControl::TextComposition {
            selector,
            updates,
            commit,
        } = operation
        else {
            unreachable!("composition queue called for a non-composition operation");
        };
        let presentation = match self.current_visible_presentation() {
            Ok(presentation) => presentation,
            Err(unsupported) => {
                return Err((native_physical_control_error(unsupported), lease, pending));
            }
        };
        let Some(region) =
            focus_region_for_native_physical_selector(&presentation, selector).cloned()
        else {
            return Err((
                native_physical_control_error(NativePhysicalControlUnsupported::new(
                    "native-physical-control-text-composition-region-not-found",
                    "the current iced visible presentation has no enabled text edit region matching the composition selector",
                )),
                lease,
                pending,
            ));
        };
        let Some(text_edit) = region.text_edit.as_ref() else {
            return Err((
                native_physical_control_error(NativePhysicalControlUnsupported::new(
                    "native-physical-control-text-composition-text-edit-required",
                    "text composition requires a selected native text edit region",
                )),
                lease,
                pending,
            ));
        };
        if !text_edit.selection.editable
            || !text_edit.edit_commands.iter().any(|command| {
                command.enabled && command.kind == slipway_core::TextEditKind::ReplaceBuffer
            })
        {
            return Err((
                native_physical_control_error(NativePhysicalControlUnsupported::new(
                    "native-physical-control-text-composition-commit-unavailable",
                    "the selected native text edit region is not editable with an enabled replace-buffer mutation command",
                )),
                lease,
                pending,
            ));
        }
        if let Err(unsupported) = self.focus_native_region_for_selector(selector) {
            return Err((native_physical_control_error(unsupported), lease, pending));
        }
        let focused_before = self
            .focused_visible_region()
            .is_some_and(|focused| focused == region.id);
        if !focused_before {
            return Err((
                native_physical_control_error(NativePhysicalControlUnsupported::new(
                    "native-physical-control-text-composition-focus-unstable",
                    "the selected native text edit region did not become focused before composition ingress",
                )),
                lease,
                pending,
            ));
        }

        let Some(window) = self.window.as_ref() else {
            return Err((
                native_physical_control_error(NativePhysicalControlUnsupported::new(
                    "native-physical-control-window-unavailable",
                    "the iced native runner cannot dispatch composition before the visible window exists",
                )),
                lease,
                pending,
            ));
        };
        let scale_factor = window.viewport.scale_factor();
        let modifiers = window.modifiers;
        let converted = (|| -> Result<Vec<iced::Event>, NativePhysicalControlUnsupported> {
            let mut events = Vec::with_capacity(updates.len() + 3);
            events.push(convert_iced_winit_event(
                WindowEvent::Ime(Ime::Enabled),
                scale_factor,
                modifiers,
                "native-physical-control-text-composition-start-conversion-failed",
            )?);
            for update in updates {
                let range = update.cursor_range.as_ref().map(|range| {
                    let start = unicode_scalar_to_byte_index(&update.preedit_text, range.anchor);
                    let end = unicode_scalar_to_byte_index(&update.preedit_text, range.focus);
                    (start, end)
                });
                events.push(convert_iced_winit_event(
                    WindowEvent::Ime(Ime::Preedit(update.preedit_text.clone(), range)),
                    scale_factor,
                    modifiers,
                    "native-physical-control-text-composition-update-conversion-failed",
                )?);
            }
            events.push(convert_iced_winit_event(
                WindowEvent::Ime(Ime::Commit(commit.clone())),
                scale_factor,
                modifiers,
                "native-physical-control-text-composition-commit-conversion-failed",
            )?);
            events.push(convert_iced_winit_event(
                WindowEvent::Ime(Ime::Disabled),
                scale_factor,
                modifiers,
                "native-physical-control-text-composition-end-conversion-failed",
            )?);
            Ok(events)
        })();
        let events = match converted {
            Ok(events) => events,
            Err(unsupported) => {
                return Err((native_physical_control_error(unsupported), lease, pending));
            }
        };

        let token = self.next_request_token();
        let commit_sequence_index = updates.len() + 1;
        self.pending_physical_control = Some(PendingIcedNativePhysicalControl {
            token,
            command,
            lease,
            pending,
            evidence: PendingIcedPhysicalEvidence::Composition {
                target: region.target.clone(),
                target_slot: region.address.clone(),
                region: region.id.clone(),
                focused_before,
                phases: Vec::with_capacity(events.len()),
                commit_mutation: None,
            },
        });
        self.pending_iced_events.reserve(events.len());
        for (sequence_index, event) in events.into_iter().enumerate() {
            let phase = if sequence_index == 0 {
                TextCompositionPhase::Start
            } else if sequence_index <= updates.len() {
                TextCompositionPhase::Update
            } else if sequence_index == commit_sequence_index {
                TextCompositionPhase::Commit
            } else {
                TextCompositionPhase::End
            };
            let commit_mutation_gate =
                (phase == TextCompositionPhase::Commit).then(|| IcedNativeCommitMutationGate {
                    token,
                    commit_sequence_index,
                    target: region.target.clone(),
                    target_slot: region.address.clone(),
                    region_id: region.id.clone(),
                });
            self.pending_iced_events.push(QueuedIcedIngress {
                event,
                origin: IcedIngressOrigin::DebugComposition {
                    token,
                    sequence_index,
                    phase,
                },
                commit_mutation_gate,
            });
        }
        if let Some(window) = self.window.as_ref() {
            window.raw.request_redraw();
        }
        Ok(())
    }

    fn run_native_physical_control(
        &mut self,
        command: &DebugCommand,
        operation: &DebugPhysicalControl,
    ) -> DebugReplyProduct {
        match operation {
            DebugPhysicalControl::Focus { selector, focused } => {
                return self.run_native_focus_control(command, selector, *focused);
            }
            DebugPhysicalControl::Command {
                selector: _,
                command: _,
                payload_ref: _,
            } => {
                return native_physical_control_error(NativePhysicalControlUnsupported::new(
                    "native-physical-control-command-unsupported",
                    "iced native command physical control has no supported visible widget operation seam",
                ));
            }
            DebugPhysicalControl::Scroll {
                selector: _,
                offset_x: _,
                offset_y: _,
            } => {
                return native_physical_control_error(NativePhysicalControlUnsupported::new(
                    "native-physical-control-scroll-unsupported",
                    "iced absolute scroll offsets are not physical-equivalent evidence because iced exposes them as widget operations, not as native winit input; use wheel so success is proven by backend-presented scroll input",
                ));
            }
            DebugPhysicalControl::Text { selector, text } => {
                return self.run_native_text_control(command, selector, text);
            }
            DebugPhysicalControl::TextEdit {
                selector,
                kind: slipway_core::TextEditKind::MoveCaret,
                selection_before,
                selection_after,
                ..
            } => {
                return self.run_native_caret_control(
                    command,
                    selector,
                    selection_before.clone(),
                    selection_after.clone(),
                );
            }
            DebugPhysicalControl::TextEdit {
                selector,
                kind,
                text,
                selection_before,
                selection_after,
            } => {
                return self.run_native_text_edit_control(
                    command,
                    selector,
                    *kind,
                    text.clone(),
                    selection_before.clone(),
                    selection_after.clone(),
                );
            }
            DebugPhysicalControl::Keyboard { selector, .. } => {
                if let Err(unsupported) = self.focus_native_region_for_selector(selector) {
                    return native_physical_control_error(unsupported);
                }
            }
            DebugPhysicalControl::TextComposition { .. } => {
                return native_physical_control_error(NativePhysicalControlUnsupported::new(
                    "native-physical-control-text-composition-queue-required",
                    "iced text composition must be admitted as one tokened native queue batch",
                ));
            }
            DebugPhysicalControl::Pointer { .. } | DebugPhysicalControl::Wheel { .. } => {}
        }
        let events = match self.physical_control_events(operation) {
            Ok(events) => events,
            Err(unsupported) => {
                return native_physical_control_error(unsupported);
            }
        };
        self.run_native_event_physical_control(command, events)
    }

    fn run_native_event_physical_control(
        &mut self,
        command: &DebugCommand,
        events: Vec<iced::Event>,
    ) -> DebugReplyProduct {
        if events.is_empty() {
            return DebugReplyProduct::Error(DebugFailure {
                code: "native-physical-control-empty-event-plan".to_string(),
                message:
                    "the iced native runner produced no native events for this physical control"
                        .to_string(),
                dispatch_evidence: None,
            });
        }
        let messages = self.dispatch_native_physical_events(&events);
        let mut backend_inputs = Vec::new();
        let mut refusals = Vec::new();
        let mut remaining = Vec::new();
        for message in messages {
            match message {
                SlipwayIcedRuntimeMessage::BackendInput(input) => backend_inputs.push(input),
                SlipwayIcedRuntimeMessage::DispatchRefusal(evidence) => refusals.push(evidence),
                other => remaining.push(other),
            }
        }
        // Retain routing-level refusal evidence (audit finding MF-H3)
        // regardless of the match outcome: the injected events were the only
        // events dispatched, so every captured refusal belongs to this
        // operation.
        for refusal in &refusals {
            self.app
                .runtime_mut()
                .record_dispatch_refusal_for_backend(refusal.clone(), ICED_BACKEND_ID);
        }

        let matched = backend_inputs
            .iter()
            .position(|input| {
                self.app
                    .runtime()
                    .backend_presented_physical_control_input_matches(command, input)
            })
            .map(|index| backend_inputs.remove(index));

        let backend_input = match matched {
            Some(backend_input) => backend_input,
            None => {
                let Some(focus_input) = self.pointer_press_focus_input_after_native_update(command)
                else {
                    // Error-path half-apply repair (audit finding MF-M18):
                    // the synthesized events already ran through
                    // UserInterface::update above, so the real input path
                    // would process every non-input runtime message they
                    // produced. Refusing the MCP command must not silently
                    // drop those messages — process them exactly as the
                    // real path would. Non-matching backend inputs are NOT
                    // applied: applying input the command failed to prove
                    // would make the refusal reply lie about runtime state.
                    let withheld_backend_inputs = backend_inputs.len();
                    self.process_messages(remaining);
                    return DebugReplyProduct::Error(native_physical_no_match_failure(
                        refusals.pop(),
                        withheld_backend_inputs,
                    ));
                };
                focus_input
            }
        };

        let product = self
            .app
            .handle_backend_presented_physical_control(command.clone(), backend_input);
        remaining.extend(
            backend_inputs
                .into_iter()
                .map(SlipwayIcedRuntimeMessage::BackendInput),
        );
        self.process_messages(remaining);
        product
    }

    fn dispatch_native_physical_events(
        &mut self,
        events: &[iced::Event],
    ) -> Vec<SlipwayIcedRuntimeMessage> {
        let mut messages = Vec::new();
        for event in events {
            messages.extend(self.dispatch_iced_events(std::slice::from_ref(event)));
        }
        messages
    }

    fn pointer_press_focus_input_after_native_update(
        &mut self,
        command: &DebugCommand,
    ) -> Option<BackendInputEvent> {
        let DebugCommandKind::PhysicalControl {
            operation:
                DebugPhysicalControl::Pointer {
                    position,
                    kind: slipway_core::PointerEventKind::Press,
                    button: Some(_),
                    ..
                },
            ..
        } = &command.kind
        else {
            return None;
        };
        let presentation = self.current_visible_presentation().ok()?;
        let selector = slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Position {
            position: *position,
        };
        let region = focus_region_for_native_physical_selector(&presentation, &selector)?.clone();
        let event = InputEvent::Focus(slipway_core::FocusEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            focused: true,
        });
        Some(backend_focus_input_event(
            &presentation,
            &region,
            DeclaredEventDispatchKind::Focus,
            Some(*position),
            event,
        ))
    }

    fn run_native_focus_control(
        &mut self,
        command: &DebugCommand,
        selector: &slipway_debug_bridge::DebugPhysicalControlDeclarationSelector,
        focused: bool,
    ) -> DebugReplyProduct {
        let presentation = match self.current_visible_presentation() {
            Ok(presentation) => presentation,
            Err(unsupported) => return native_physical_control_error(unsupported),
        };
        let Some(region) =
            focus_region_for_native_physical_selector(&presentation, selector).cloned()
        else {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-focus-region-not-found",
                "the current iced visible presentation has no enabled focus region matching the physical control selector",
            ));
        };
        let Some(id) = iced_focus_widget_id_for_region(&region) else {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-focus-native-widget-unavailable",
                "the selected focus region is not backed by a mounted native iced focusable widget",
            ));
        };
        let operation_result = if focused {
            let mut operation = iced::advanced::widget::operation::focusable::focus(id);
            self.operate_visible_ui(&mut operation)
        } else {
            let mut operation = iced::advanced::widget::operation::focusable::unfocus();
            self.operate_visible_ui(&mut operation)
        };
        if let Err(unsupported) = operation_result {
            return native_physical_control_error(unsupported);
        }
        let backend_input = backend_native_focus_input_event(&presentation, &region, focused);
        self.app
            .handle_backend_presented_physical_control(command.clone(), backend_input)
    }

    fn run_native_caret_control(
        &mut self,
        command: &DebugCommand,
        selector: &slipway_debug_bridge::DebugPhysicalControlDeclarationSelector,
        selection_before: Option<slipway_core::TextSelectionRange>,
        selection_after: Option<slipway_core::TextSelectionRange>,
    ) -> DebugReplyProduct {
        let presentation = match self.current_visible_presentation() {
            Ok(presentation) => presentation,
            Err(unsupported) => return native_physical_control_error(unsupported),
        };
        let Some(region) =
            focus_region_for_native_physical_selector(&presentation, selector).cloned()
        else {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-caret-region-not-found",
                "the current iced visible presentation has no enabled text edit focus region matching the physical control selector",
            ));
        };
        let Some(text_edit) = region.text_edit.as_ref() else {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-caret-text-edit-required",
                "caret movement requires a selected text edit focus region",
            ));
        };
        if text_edit.line_mode != slipway_core::TextLineMode::SingleLine {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-caret-multiline-unsupported",
                "iced exposes public text input cursor operations for single-line TextInput only; multiline TextEditor caret movement is left unsupported instead of being faked",
            ));
        }
        let caret = selection_after
            .as_ref()
            .map(|range| range.focus)
            .or_else(|| selection_after.as_ref().map(|range| range.anchor))
            .unwrap_or_else(|| text_edit.buffer.text.chars().count());
        let mut operation = iced::advanced::widget::operation::text_input::move_cursor_to(
            iced_text_input_id(&region),
            caret,
        );
        if let Err(unsupported) = self.operate_visible_ui(&mut operation) {
            return native_physical_control_error(unsupported);
        }
        let backend_input = backend_native_text_caret_input_event(
            &presentation,
            &region,
            selection_before,
            selection_after,
        );
        self.app
            .handle_backend_presented_physical_control(command.clone(), backend_input)
    }

    fn run_native_text_control(
        &mut self,
        command: &DebugCommand,
        selector: &slipway_debug_bridge::DebugPhysicalControlDeclarationSelector,
        text: &str,
    ) -> DebugReplyProduct {
        let presentation = match self.current_visible_presentation() {
            Ok(presentation) => presentation,
            Err(unsupported) => return native_physical_control_error(unsupported),
        };
        let Some(region) =
            focus_region_for_native_physical_selector(&presentation, selector).cloned()
        else {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-text-region-not-found",
                "the current iced visible presentation has no enabled text focus region matching the physical control selector",
            ));
        };
        if region.text_edit.is_none() {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-text-edit-region-required",
                "text input requires a selected focus region backed by a TextEditRegionDeclaration",
            ));
        }
        let event = InputEvent::Text(slipway_core::TextInputEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            text: text.to_string(),
        });
        let backend_input = backend_focus_input_event(
            &presentation,
            &region,
            DeclaredEventDispatchKind::Text,
            None,
            event,
        );
        self.app
            .handle_backend_presented_physical_control(command.clone(), backend_input)
    }

    fn run_native_text_edit_control(
        &mut self,
        command: &DebugCommand,
        selector: &slipway_debug_bridge::DebugPhysicalControlDeclarationSelector,
        kind: slipway_core::TextEditKind,
        text: Option<String>,
        selection_before: Option<slipway_core::TextSelectionRange>,
        selection_after: Option<slipway_core::TextSelectionRange>,
    ) -> DebugReplyProduct {
        let presentation = match self.current_visible_presentation() {
            Ok(presentation) => presentation,
            Err(unsupported) => return native_physical_control_error(unsupported),
        };
        let Some(region) =
            focus_region_for_native_physical_selector(&presentation, selector).cloned()
        else {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-text-edit-region-not-found",
                "the current iced visible presentation has no enabled text edit focus region matching the physical control selector",
            ));
        };
        let Some(text_edit) = region.text_edit.as_ref() else {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-text-edit-region-required",
                "text edit requires a selected focus region backed by a TextEditRegionDeclaration",
            ));
        };
        if !text_edit
            .edit_commands
            .iter()
            .any(|command| command.enabled && command.kind == kind)
        {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-text-edit-command-unavailable",
                "the selected text edit region does not declare an enabled command for the requested edit kind",
            ));
        }
        let event = InputEvent::TextEdit(slipway_core::TextEditEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            kind,
            text,
            selection_before,
            selection_after,
        });
        let backend_input = backend_focus_input_event(
            &presentation,
            &region,
            DeclaredEventDispatchKind::Text,
            None,
            event,
        );
        self.app
            .handle_backend_presented_physical_control(command.clone(), backend_input)
    }

    fn focus_native_region_for_selector(
        &mut self,
        selector: &slipway_debug_bridge::DebugPhysicalControlDeclarationSelector,
    ) -> Result<(), NativePhysicalControlUnsupported> {
        let presentation = self.current_visible_presentation()?;
        let Some(region) =
            focus_region_for_native_physical_selector(&presentation, selector).cloned()
        else {
            return Err(NativePhysicalControlUnsupported::new(
                "native-physical-control-text-focus-region-not-found",
                "the current iced visible presentation has no enabled text focus region matching the physical control selector",
            ));
        };
        let Some(id) = iced_focus_widget_id_for_region(&region) else {
            return Err(NativePhysicalControlUnsupported::new(
                "native-physical-control-text-focus-widget-unavailable",
                "the selected focus region is not backed by a native iced text input/editor widget",
            ));
        };
        let mut operation = iced::advanced::widget::operation::focusable::focus(id);
        self.operate_visible_ui(&mut operation)
    }

    fn current_visible_presentation(
        &mut self,
    ) -> Result<IcedPresentationState, NativePhysicalControlUnsupported> {
        if self.window.is_none() {
            return Err(NativePhysicalControlUnsupported::new(
                "native-physical-control-window-unavailable",
                "the iced native runner cannot inspect the visible presentation before the visible window exists",
            ));
        }
        self.sync_presented_viewport_from_window();
        let frame = self.app.runtime().last_frame_identity();
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(frame.viewport),
            content: TargetLocalRect::new(frame.viewport),
            constraints: LayoutConstraints {
                min: slipway_core::Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame.viewport.size,
            },
        };
        Ok(iced_presentation_for_widget(
            self.app.runtime().widget(),
            self.app.runtime().external(),
            self.app.runtime().local_state(),
            layout_input,
            Some(&frame),
            0,
            None,
        ))
    }

    fn operate_visible_ui(
        &mut self,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) -> Result<(), NativePhysicalControlUnsupported> {
        let Some(window) = self.window.as_mut() else {
            return Err(NativePhysicalControlUnsupported::new(
                "native-physical-control-window-unavailable",
                "the iced native runner cannot operate the visible UI before the visible window exists",
            ));
        };
        let cache = std::mem::replace(&mut window.cache, user_interface::Cache::new());
        let mut user_interface = UserInterface::build(
            self.app.view::<iced::Theme, iced::Renderer>(),
            window.viewport.logical_size(),
            cache,
            &mut window.renderer,
        );
        user_interface.operate(&window.renderer, operation);
        window.cache = user_interface.into_cache();
        window.raw.request_redraw();
        Ok(())
    }

    fn focused_visible_region(&mut self) -> Option<PresentationRegionId> {
        let mut focused = None;
        let mut operation = iced_focused_region_query(&mut focused);
        self.operate_visible_ui(&mut operation).ok()?;
        focused
    }

    fn refresh_visible_ui_cache_after_physical_control(&mut self) {
        // Iced stores native widget state such as Scrollable offsets in the
        // UI cache. Replacing it here makes the next physical-control probe
        // dispatch against a freshly-built top-of-scroll tree instead of the
        // backend-presented state the previous wheel event just produced.
    }

    fn physical_control_events(
        &mut self,
        operation: &DebugPhysicalControl,
    ) -> Result<Vec<iced::Event>, NativePhysicalControlUnsupported> {
        let Some(window) = self.window.as_mut() else {
            return Err(NativePhysicalControlUnsupported::new(
                "native-physical-control-window-unavailable",
                "the iced native runner cannot dispatch physical input before the visible window exists",
            ));
        };
        let scale_factor = window.viewport.scale_factor();
        match operation {
            DebugPhysicalControl::Pointer { position, .. }
            | DebugPhysicalControl::Wheel { position, .. } => {
                let physical = PhysicalPosition::new(
                    f64::from(position.x * scale_factor),
                    f64::from(position.y * scale_factor),
                );
                window.cursor_position = Some(physical);
            }
            // Mirror the real path's WindowEvent::ModifiersChanged
            // bookkeeping (audit finding MF-M18): a real key event is
            // preceded by a ModifiersChanged reflecting the declared
            // modifier state, and subsequent window-event conversion reads
            // that side-state. Without the mirror, seam keyboard input
            // interleaved with real input diverges from pure-real behavior.
            DebugPhysicalControl::Keyboard { modifiers, .. } => {
                window.modifiers = winit_modifiers_state_from_slipway(*modifiers);
            }
            _ => {}
        }
        let events =
            iced_events_for_native_physical_operation(operation, scale_factor, window.modifiers)?;
        // Mirror the real path's WindowEvent::MouseInput bookkeeping (audit
        // finding MF-M18) for the seam-synthesized MouseInput this plan
        // contains: `pressed_mouse_buttons` gates the real path's
        // cursor-only deferral, so a seam press followed by real cursor
        // motion must observe the same side-state a real press leaves.
        window.pressed_mouse_buttons =
            seam_pressed_mouse_buttons_after(window.pressed_mouse_buttons, operation);
        Ok(events)
    }
}

/// Side-state mirror for seam-synthesized MouseInput (audit finding
/// MF-M18): returns the `pressed_mouse_buttons` count after the given
/// physical operation, matching the real winit path's
/// `WindowEvent::MouseInput` bookkeeping (`Pressed` saturating-adds,
/// `Released` saturating-subs, everything else leaves the count alone).
fn seam_pressed_mouse_buttons_after(pressed: u8, operation: &DebugPhysicalControl) -> u8 {
    match operation {
        DebugPhysicalControl::Pointer {
            kind: slipway_core::PointerEventKind::Press,
            ..
        } => pressed.saturating_add(1),
        DebugPhysicalControl::Pointer {
            kind: slipway_core::PointerEventKind::Release,
            ..
        } => pressed.saturating_sub(1),
        _ => pressed,
    }
}

/// Maps declared Slipway modifiers onto the winit `ModifiersState` the real
/// ingress path tracks via `WindowEvent::ModifiersChanged` (audit finding
/// MF-M18), so seam keyboard operations leave the same modifier side-state
/// a real modifier sequence would.
fn winit_modifiers_state_from_slipway(modifiers: slipway_core::Modifiers) -> ModifiersState {
    let mut state = ModifiersState::empty();
    if modifiers.shift {
        state |= ModifiersState::SHIFT;
    }
    if modifiers.control {
        state |= ModifiersState::CONTROL;
    }
    if modifiers.alt {
        state |= ModifiersState::ALT;
    }
    if modifiers.meta {
        state |= ModifiersState::SUPER;
    }
    state
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

fn native_physical_control_error(
    unsupported: NativePhysicalControlUnsupported,
) -> DebugReplyProduct {
    DebugReplyProduct::Error(DebugFailure {
        code: unsupported.code.to_string(),
        message: unsupported.message.to_string(),
        dispatch_evidence: None,
    })
}

/// Builds the seam no-match refusal (audit findings MF-M18 + MF-H5).
///
/// The message states that the synthesized events WERE dispatched — the
/// widget tree already updated before backend-input matching, so cursor,
/// hover, scroll, and focus side effects may have advanced even though the
/// operation is refused — and how many non-matching backend input messages
/// were withheld from the runtime. When routing constructed refusal
/// evidence for the operation, it is attached under the distinct
/// `post_hoc_diagnosis` source label so the agent can see WHY the operation
/// was dead (position, candidates, reason) without ever confusing the
/// diagnosis with real dispatch evidence.
fn native_physical_no_match_failure(
    refusal: Option<slipway_core::DeclaredEventDispatchEvidence>,
    withheld_backend_inputs: usize,
) -> DebugFailure {
    let diagnosis = refusal.map(|mut refusal| {
        refusal.source = slipway_core::EvidenceSource::post_hoc_diagnosis(
            ICED_BACKEND_ID,
            "physical-control-no-match",
        );
        refusal
    });
    let dispatched = format!(
        "the synthesized iced native events were dispatched through UserInterface::update before matching — native widget state (cursor, hover, scroll, focus) may have advanced, and non-input runtime messages were still processed to preserve real-input semantics — but no backend-presented input evidence matched the requested physical operation; {withheld_backend_inputs} non-matching backend input message(s) were withheld from the runtime"
    );
    let message = match &diagnosis {
        Some(diagnosis) => format!(
            "{dispatched}; a post-hoc dispatch diagnosis (source label `{}`) is attached: {}; candidates=[{}]",
            slipway_core::EVIDENCE_SOURCE_POST_HOC_DIAGNOSIS,
            diagnosis
                .refusal_reason
                .as_deref()
                .unwrap_or("dispatch refused without a recorded reason"),
            diagnosis
                .candidate_regions
                .iter()
                .map(|region| region.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        None => format!(
            "{dispatched}; no routing-level refusal evidence was constructed for it — probe the `diagnostics` kind for retained refusals"
        ),
    };
    DebugFailure {
        code: "native-physical-control-produced-no-backend-input".to_string(),
        message,
        dispatch_evidence: diagnosis,
    }
}

pub(super) fn iced_events_for_native_physical_operation(
    operation: &DebugPhysicalControl,
    scale_factor: f32,
    modifiers: ModifiersState,
) -> Result<Vec<iced::Event>, NativePhysicalControlUnsupported> {
    match operation {
        DebugPhysicalControl::Pointer {
            position,
            kind,
            button,
            ..
        } => {
            let physical = PhysicalPosition::new(
                f64::from(position.x * scale_factor),
                f64::from(position.y * scale_factor),
            );
            let mut events = Vec::new();
            events.push(convert_iced_winit_event(
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: physical,
                },
                scale_factor,
                modifiers,
                "native-physical-control-pointer-move-conversion-failed",
            )?);
            match kind {
                slipway_core::PointerEventKind::Press | slipway_core::PointerEventKind::Release => {
                    events.push(convert_iced_winit_event(
                        WindowEvent::MouseInput {
                            device_id: DeviceId::dummy(),
                            state: match kind {
                                slipway_core::PointerEventKind::Press => ElementState::Pressed,
                                _ => ElementState::Released,
                            },
                            button: button
                                .map(debug_pointer_button)
                                .unwrap_or(MouseButton::Left),
                        },
                        scale_factor,
                        modifiers,
                        "native-physical-control-pointer-button-conversion-failed",
                    )?);
                }
                slipway_core::PointerEventKind::Move => {}
                slipway_core::PointerEventKind::Enter | slipway_core::PointerEventKind::Leave => {
                    return Err(NativePhysicalControlUnsupported::new(
                        "native-physical-control-pointer-hover-unsupported",
                        "iced derives pointer enter/leave from cursor state; request a pointer move to the target position instead",
                    ));
                }
                slipway_core::PointerEventKind::Cancel => {
                    return Err(NativePhysicalControlUnsupported::new(
                        "native-physical-control-pointer-cancel-unsupported",
                        "iced_winit has no mouse cancellation event for this runner path",
                    ));
                }
            }
            Ok(events)
        }
        DebugPhysicalControl::Wheel {
            position,
            delta_x,
            delta_y,
        } => {
            let physical = PhysicalPosition::new(
                f64::from(position.x * scale_factor),
                f64::from(position.y * scale_factor),
            );
            Ok(vec![
                convert_iced_winit_event(
                    WindowEvent::CursorMoved {
                        device_id: DeviceId::dummy(),
                        position: physical,
                    },
                    scale_factor,
                    modifiers,
                    "native-physical-control-wheel-move-conversion-failed",
                )?,
                convert_iced_winit_event(
                    WindowEvent::MouseWheel {
                        device_id: DeviceId::dummy(),
                        delta: MouseScrollDelta::LineDelta(*delta_x, *delta_y),
                        phase: TouchPhase::Moved,
                    },
                    scale_factor,
                    modifiers,
                    "native-physical-control-wheel-conversion-failed",
                )?,
            ])
        }
        DebugPhysicalControl::Text { text, .. } => Ok(vec![convert_iced_winit_event(
            WindowEvent::Ime(Ime::Commit(text.clone())),
            scale_factor,
            modifiers,
            "native-physical-control-text-conversion-failed",
        )?]),
        DebugPhysicalControl::TextEdit { kind, text, .. } => match kind {
            slipway_core::TextEditKind::InsertText
            | slipway_core::TextEditKind::ReplaceSelection
            | slipway_core::TextEditKind::ReplaceBuffer => {
                let Some(text) = text else {
                    return Err(NativePhysicalControlUnsupported::new(
                        "native-physical-control-text-edit-text-required",
                        "iced native text edit insertion requires a text payload; IME composition itself is left to iced/winit",
                    ));
                };
                Ok(vec![convert_iced_winit_event(
                    WindowEvent::Ime(Ime::Commit(text.clone())),
                    scale_factor,
                    modifiers,
                    "native-physical-control-text-edit-conversion-failed",
                )?])
            }
            slipway_core::TextEditKind::DeleteBackward => Ok(vec![iced_keyboard_event(
                "Backspace",
                slipway_core::KeyEventKind::Press,
                slipway_core::Modifiers::default(),
                &slipway_core::KeyboardDetails::default(),
            )]),
            slipway_core::TextEditKind::DeleteForward => Ok(vec![iced_keyboard_event(
                "Delete",
                slipway_core::KeyEventKind::Press,
                slipway_core::Modifiers::default(),
                &slipway_core::KeyboardDetails::default(),
            )]),
            slipway_core::TextEditKind::MoveCaret | slipway_core::TextEditKind::Unknown => {
                Err(NativePhysicalControlUnsupported::new(
                    "native-physical-control-text-edit-kind-unsupported",
                    "caret motion is handled by the iced widget operation seam; unknown text edit kinds are refused",
                ))
            }
        },
        DebugPhysicalControl::TextComposition { .. } => Err(NativePhysicalControlUnsupported::new(
            "native-physical-control-text-composition-atomic-queue-required",
            "text composition is converted as one request-scoped Enabled/Preedit/Commit/Disabled queue batch",
        )),
        DebugPhysicalControl::Keyboard {
            key,
            kind,
            modifiers,
            details,
            ..
        } => Ok(vec![iced_keyboard_event(key, *kind, *modifiers, details)]),
        DebugPhysicalControl::Focus { .. } => Err(NativePhysicalControlUnsupported::new(
            "native-physical-control-focus-unsupported",
            "Slipway focus-region focus is not a winit window focus event; iced focus requires a backend widget operation seam",
        )),
        DebugPhysicalControl::Command { .. } => Err(NativePhysicalControlUnsupported::new(
            "native-physical-control-command-unsupported",
            "arbitrary Slipway commands are not winit events; iced command support requires a backend-presented command seam",
        )),
        DebugPhysicalControl::Scroll { .. } => Err(NativePhysicalControlUnsupported::new(
            "native-physical-control-scroll-unsupported",
            "absolute scroll offsets are iced scrollable state operations, not winit wheel events; use wheel or add a backend scroll operation seam",
        )),
    }
}

fn convert_iced_winit_event(
    event: WindowEvent,
    scale_factor: f32,
    modifiers: ModifiersState,
    code: &'static str,
) -> Result<iced::Event, NativePhysicalControlUnsupported> {
    iced_winit::conversion::window_event(event, scale_factor, modifiers).ok_or_else(|| {
        NativePhysicalControlUnsupported::new(
            code,
            "iced_winit refused to convert the synthesized native window event",
        )
    })
}

fn iced_keyboard_event(
    key: &str,
    kind: slipway_core::KeyEventKind,
    modifiers: slipway_core::Modifiers,
    details: &slipway_core::KeyboardDetails,
) -> iced::Event {
    let logical = details.logical_key.as_deref().unwrap_or(key);
    let key = iced_keyboard_key_from_label(logical);
    let modified_key = key.clone();
    let physical_key =
        iced_physical_key_from_label(details.physical_key.as_deref().or(Some(logical)));
    let location = iced_key_location_from_slipway(details.location);
    let modifiers = iced_modifiers_from_slipway(modifiers);

    iced::Event::Keyboard(match kind {
        slipway_core::KeyEventKind::Press => iced::keyboard::Event::KeyPressed {
            key,
            modified_key,
            physical_key,
            location,
            modifiers,
            text: details.text.clone().map(Into::into),
            repeat: details.repeat,
        },
        slipway_core::KeyEventKind::Release => iced::keyboard::Event::KeyReleased {
            key,
            modified_key,
            physical_key,
            location,
            modifiers,
        },
    })
}

impl<W, F> ApplicationHandler<NativeRunnerEvent> for NativeIcedRunner<W, F>
where
    W: SlipwayIcedBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: Clone + std::fmt::Debug + Send + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.create_window(event_loop).is_err() {
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: NativeRunnerEvent) {
        match event {
            NativeRunnerEvent::McpWake => self.pump_mcp(),
            NativeRunnerEvent::DirectCaptureWake { token } => {
                self.handle_direct_capture_wake(token)
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.handle_window_event(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(transport) = self.app.debug_mcp_transport.as_ref() {
            if transport.drain_wakes() > 0 {
                self.pump_mcp();
            }
        }
        let now = Instant::now();
        if let Some(due_at) = self.pending_resize_due_at() {
            if due_at > now {
                event_loop.set_control_flow(ControlFlow::WaitUntil(due_at));
                return;
            }
        }
        self.apply_pending_resize(true);
        let pending_cursor_only = should_defer_pending_cursor_only(
            &self.pending_iced_events,
            self.window
                .as_ref()
                .map_or(0, |window| window.pressed_mouse_buttons),
        );
        let dispatched_pending_input = !pending_cursor_only && !self.pending_iced_events.is_empty();
        if !pending_cursor_only {
            self.dispatch_pending_iced_events();
        }
        let has_pending_cursor_only = pending_cursor_only && !self.pending_iced_events.is_empty();
        let Some(window) = self.window.as_mut() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        if window.presented_frames == 0 {
            window.raw.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        if has_pending_cursor_only {
            window.raw.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        if dispatched_pending_input {
            // Queued semantic input (wheel included) was flushed at this
            // iteration boundary; keep the previous immediate-flush contract
            // of one redraw request after dispatch.
            window.raw.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        let Some(redraw_at) = window.redraw_at else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        if redraw_at <= now {
            window.redraw_at = None;
            window.raw.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(redraw_at));
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.refuse_pending_presented_capture_on_teardown();
    }
}

fn non_zero_physical_size(size: PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}

fn load_platform_text_fonts(
    compositor: &mut iced_renderer::Compositor,
    ime_trace: bool,
    frame_timing: &mut VisibleFrameTimingRecorder,
) -> usize {
    #[cfg(target_os = "windows")]
    {
        const CANDIDATES: &[&str] = &[
            "C:\\Windows\\Fonts\\NotoSansKR-VF.ttf",
            "C:\\Windows\\Fonts\\malgun.ttf",
            "C:\\Windows\\Fonts\\gulim.ttc",
        ];

        for path in CANDIDATES {
            let read_start = Instant::now();
            let bytes = match std::fs::read(path) {
                Ok(bytes) => {
                    frame_timing.record(
                        "iced.platform_font.read",
                        read_start.elapsed(),
                        bytes.len(),
                        None,
                    );
                    bytes
                }
                Err(_) => {
                    frame_timing.record("iced.platform_font.read", read_start.elapsed(), 0, None);
                    continue;
                }
            };
            let byte_count = bytes.len();
            let load_start = Instant::now();
            compositor.load_font(Cow::Owned(bytes));
            frame_timing.record(
                "iced.platform_font.load",
                load_start.elapsed(),
                byte_count,
                None,
            );
            trace_ime(ime_trace, format_args!("loaded platform text font {path}"));
            return 1;
        }

        trace_ime(
            ime_trace,
            "no Windows Korean platform font candidate was available",
        );
        0
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = compositor;
        let _ = ime_trace;
        let _ = frame_timing;
        0
    }
}

fn viewport_from_window(size: &PhysicalSize<u32>, scale_factor: f32) -> Viewport {
    let physical_size = non_zero_physical_size(*size);
    Viewport::with_physical_size(
        iced::Size::new(physical_size.width, physical_size.height),
        scale_factor,
    )
}

fn iced_timing_viewport(window: &NativeIcedWindow) -> Size {
    let logical = window.viewport.logical_size();
    Size {
        width: logical.width,
        height: logical.height,
    }
}

fn ensure_window_ime_policy(window: &mut NativeIcedWindow, reason: &'static str) {
    if window.ime_policy.keeps_platform_ime_allowed() {
        set_window_ime_allowed(window, true, reason);
    }
}

fn set_window_ime_allowed(
    window: &mut NativeIcedWindow,
    allowed: bool,
    reason: impl std::fmt::Display,
) {
    if window.ime_allowed == allowed {
        let _ = reason;
        return;
    }
    window.raw.set_ime_allowed(allowed);
    window.ime_allowed = allowed;
    trace_ime(
        window.ime_trace,
        format_args!("{reason} set_ime_allowed({allowed})"),
    );
}

fn request_window_input_method(
    window: &mut NativeIcedWindow,
    input_method: iced_core::InputMethod,
) {
    match input_method {
        iced_core::InputMethod::Disabled => {
            if window.ime_policy.keeps_platform_ime_allowed() {
                set_window_ime_allowed(window, true, "iced InputMethod::Disabled");
                if window.ime_composing {
                    trace_ime(
                        window.ime_trace,
                        "iced disabled ignored during active platform IME composition",
                    );
                }
                return;
            }
            trace_ime(window.ime_trace, "iced InputMethod::Disabled");
            if window.ime_state.is_some() {
                set_window_ime_allowed(window, false, "iced InputMethod::Disabled");
                window.ime_state = None;
            }
        }
        iced_core::InputMethod::Enabled {
            cursor,
            purpose,
            preedit,
        } => {
            let ime_state_changed = window.ime_state != Some((cursor, purpose));
            let has_visible_preedit = preedit
                .as_ref()
                .is_some_and(|preedit| !preedit.content.is_empty());
            if ime_state_changed || has_visible_preedit {
                trace_ime(
                    window.ime_trace,
                    format_args!(
                        "iced InputMethod::Enabled cursor=({}, {}, {}, {}) purpose={:?} preedit={:?}",
                        cursor.x, cursor.y, cursor.width, cursor.height, purpose, preedit
                    ),
                );
            }
            if window.ime_state.is_none() {
                set_window_ime_allowed(window, true, "iced InputMethod::Enabled");
                trace_ime(
                    window.ime_trace,
                    "iced InputMethod::Enabled keeps platform IME allowed",
                );
            }
            if window.ime_state != Some((cursor, purpose)) {
                window.raw.set_ime_cursor_area(
                    LogicalPosition::new(cursor.x, cursor.y),
                    LogicalSize::new(cursor.width, cursor.height),
                );
                window
                    .raw
                    .set_ime_purpose(iced_winit::conversion::ime_purpose(purpose));
                window.ime_state = Some((cursor, purpose));
            }
            match preedit {
                Some(preedit) if !preedit.content.is_empty() => {
                    let Some(text_color) = window.preedit_text_color else {
                        trace_ime(
                            window.ime_trace,
                            "iced preedit skipped because explicit Slipway text input visual style is missing",
                        );
                        return;
                    };
                    let Some(font) = window.preedit_font else {
                        trace_ime(
                            window.ime_trace,
                            "iced preedit skipped because explicit Slipway text input typography is missing",
                        );
                        return;
                    };
                    let Some(text_size) = window.preedit_text_size else {
                        trace_ime(
                            window.ime_trace,
                            "iced preedit skipped because explicit Slipway text input typography size is missing",
                        );
                        return;
                    };
                    window
                        .preedit
                        .get_or_insert_with(NativePreedit::new)
                        .update(
                            cursor,
                            &preedit,
                            text_color,
                            font,
                            text_size,
                            &window.renderer,
                        );
                }
                Some(_) => {
                    window.preedit = None;
                }
                None if !window.ime_composing => {
                    window.preedit = None;
                }
                None => {}
            }
        }
    }
}

fn draw_window_preedit(window: &mut NativeIcedWindow) {
    let Some(preedit) = window.preedit.as_ref() else {
        return;
    };
    let Some(color) = window.preedit_text_color else {
        trace_ime(
            window.ime_trace,
            "iced preedit draw skipped because explicit Slipway text input visual style is missing",
        );
        return;
    };
    if window.preedit_font.is_none() || window.preedit_text_size.is_none() {
        trace_ime(
            window.ime_trace,
            "iced preedit draw skipped because explicit Slipway text input typography is missing",
        );
        return;
    }
    let viewport = iced::Rectangle::new(iced::Point::ORIGIN, window.viewport.logical_size());
    preedit.draw(&mut window.renderer, color, &viewport);
}

fn trace_iced_events_for_ime(trace: bool, events: &[iced::Event]) {
    if !trace {
        return;
    }
    for event in events {
        match event {
            iced::Event::InputMethod(event) => {
                trace_ime(true, format_args!("iced Event::InputMethod::{event:?}"));
            }
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key,
                modified_key,
                text,
                ..
            }) => {
                trace_ime(
                    true,
                    format_args!(
                        "iced Event::Keyboard::KeyPressed key={key:?} modified={modified_key:?} text={text:?}"
                    ),
                );
            }
            _ => {}
        }
    }
}

fn trace_ime(trace: bool, message: impl std::fmt::Display) {
    if trace {
        let line = format!("[slipway-ime] {message}");
        if let Some(path) = std::env::var_os("SLIPWAY_IME_TRACE_FILE") {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = std::io::Write::write_all(&mut file, line.as_bytes());
                let _ = std::io::Write::write_all(&mut file, b"\n");
            }
        } else {
            eprintln!("{line}");
        }
    }
}

fn iced_runtime_update_requires_redraw(
    update: &SlipwayIcedRuntimeAppUpdate,
    trace_requires_redraw: bool,
) -> bool {
    if update.debug_error.is_some() {
        return true;
    }
    match update.runtime_update.as_ref() {
        Some(SlipwayIcedRuntimeUpdate::Input {
            handled,
            applied_messages,
            diagnostics,
        }) => {
            if !*handled {
                return !diagnostics.is_empty();
            }
            *applied_messages > 0 || !diagnostics.is_empty() || trace_requires_redraw
        }
        Some(SlipwayIcedRuntimeUpdate::DrainDebug) => update.debug_replies_drained > 0,
        Some(SlipwayIcedRuntimeUpdate::Noop) | None => false,
    }
}

fn request_window_redraw(window: &mut NativeIcedWindow, request: iced::window::RedrawRequest) {
    match redraw_request_action(request, window.redraw_at) {
        RedrawRequestAction::RequestNow => {
            window.redraw_at = None;
            window.raw.request_redraw();
        }
        RedrawRequestAction::ScheduleAt(at) => {
            window.redraw_at = Some(at);
        }
        RedrawRequestAction::Keep => {}
    }
}

/// How the runner honors the `window::RedrawRequest` a `UserInterface::update`
/// returns, mirroring iced_winit's `Window::request_redraw`: `NextFrame`
/// requests an immediate winit redraw, `At(_)` goes through the
/// `redraw_at`/`ControlFlow::WaitUntil` machinery in `about_to_wait`, and
/// `Wait` requests nothing so widget redraw ticks cannot become a repaint
/// storm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RedrawRequestAction {
    RequestNow,
    ScheduleAt(Instant),
    Keep,
}

fn redraw_request_action(
    request: iced::window::RedrawRequest,
    scheduled_redraw_at: Option<Instant>,
) -> RedrawRequestAction {
    match request {
        iced::window::RedrawRequest::NextFrame => RedrawRequestAction::RequestNow,
        iced::window::RedrawRequest::At(at) => {
            if scheduled_redraw_at.is_none_or(|scheduled| at < scheduled) {
                RedrawRequestAction::ScheduleAt(at)
            } else {
                RedrawRequestAction::Keep
            }
        }
        iced::window::RedrawRequest::Wait => RedrawRequestAction::Keep,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RedrawFrameFlow {
    Continue,
    Exit,
}

/// Splits a drained pending-input queue (Step 181) into the event slices a
/// flush dispatches, in queue order. Each slice gets its own
/// `UserInterface::build`/`update`, and the messages it emits are applied to
/// the runtime before the next slice is dispatched.
///
/// One slice per event is the only correct split: `UserInterface::update`
/// collects messages for a whole slice without attributing them to
/// individual events, so any batching can stamp a later event's dispatch
/// evidence with a frame revision that an earlier event's message is about
/// to bump — the runtime then refuses the later input with
/// `BACKEND_INPUT_DISPATCH_EVIDENCE_FRAME_MISMATCH` and a press or captured
/// drag move is silently dropped. Adjacent coalescing already collapses
/// wheel and cursor bursts into single events, so the common flush is one
/// slice and keeps the single-build fast path.
fn pending_dispatch_slices(events: Vec<QueuedIcedIngress>) -> Vec<QueuedIcedDispatchSlice> {
    events
        .into_iter()
        .map(|queued| QueuedIcedDispatchSlice {
            events: vec![queued.event],
            origin: queued.origin,
            commit_mutation_gate: queued.commit_mutation_gate,
        })
        .collect()
}

/// iced_winit re-runs the redraw update on the same interface when it
/// invalidated layout without emitting messages, capped at three updates per
/// frame; messages instead break out to the message-application flow.
fn should_re_tick_redraw_update(
    messages_is_empty: bool,
    has_layout_changed: bool,
    redraw_updates_delivered: usize,
) -> bool {
    const MAX_REDRAW_EVENT_UPDATES_PER_FRAME: usize = 3;
    messages_is_empty
        && has_layout_changed
        && redraw_updates_delivered < MAX_REDRAW_EVENT_UPDATES_PER_FRAME
}

/// The recovery action for a failed `Compositor::present`, matching the
/// iced_winit reference: `OutOfMemory` is unrecoverable (the runner exits the
/// event loop like fatal window creation), `Lost` recreates the surface,
/// `Outdated` reconfigures it, and `Timeout`/`Other` retry on the next frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentErrorRecovery {
    Exit,
    RecreateSurface,
    ReconfigureSurface,
    RetryNextFrame,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IcedPresentIdentityOverflow;

fn iced_capture_frame_is_exact_next(expected: &FrameIdentity, captured: &FrameIdentity) -> bool {
    expected.frame_index.checked_add(1) == Some(captured.frame_index)
        && expected.surface_id == captured.surface_id
        && expected.surface_instance_id == captured.surface_instance_id
        && expected.revision == captured.revision
        && expected.viewport == captured.viewport
}

fn preflight_next_iced_present_index(
    backend_present_index: u64,
) -> Result<u64, IcedPresentIdentityOverflow> {
    backend_present_index
        .checked_add(1)
        .ok_or(IcedPresentIdentityOverflow)
}

fn commit_iced_successful_present(
    mut candidate: FrameIdentity,
    precomputed_present_index: u64,
    backend_present_index: &mut u64,
    last_successfully_presented: &mut Option<FrameIdentity>,
) {
    candidate.frame_index = precomputed_present_index;
    *backend_present_index = precomputed_present_index;
    *last_successfully_presented = Some(candidate);
}

fn finish_iced_present(
    result: Result<(), compositor::SurfaceError>,
    render_candidate: FrameIdentity,
    precomputed_present_index: u64,
    backend_present_index: &mut u64,
    last_successfully_presented: &mut Option<FrameIdentity>,
) -> IcedPresentationResult {
    match result {
        Ok(()) => {
            commit_iced_successful_present(
                render_candidate,
                precomputed_present_index,
                backend_present_index,
                last_successfully_presented,
            );
            IcedPresentationResult::Presented
        }
        Err(error) => IcedPresentationResult::CompositorFailed(error),
    }
}

#[allow(clippy::too_many_arguments)]
fn present_iced_window(
    window: &mut NativeIcedWindow,
    render_candidate: FrameIdentity,
    backend_present_index: &mut u64,
    last_successfully_presented: &mut Option<FrameIdentity>,
    pending_capture: Option<PendingPresentedCapture>,
    background_color: iced::Color,
    on_pre_present: impl FnOnce(),
) -> IcedPresentOutcome {
    let Ok(precomputed_present_index) = preflight_next_iced_present_index(*backend_present_index)
    else {
        let (pending_capture, capture_failure) = match pending_capture {
            Some(PendingPresentedCapture::Armed(capture)) => (
                None,
                Some((
                    capture,
                    "screenshot-frame-index-overflow",
                    "the iced backend presentation frame index overflowed before presentation",
                )),
            ),
            waiting => (waiting, None),
        };
        return IcedPresentOutcome {
            result: IcedPresentationResult::SkippedOverflow,
            pending_capture,
            capture_failure,
        };
    };

    let Some(pending_capture) = pending_capture else {
        let result = window.compositor.present(
            &mut window.renderer,
            &mut window.surface,
            &window.viewport,
            background_color,
            on_pre_present,
        );
        return IcedPresentOutcome {
            result: finish_iced_present(
                result,
                render_candidate,
                precomputed_present_index,
                backend_present_index,
                last_successfully_presented,
            ),
            pending_capture: None,
            capture_failure: None,
        };
    };

    let PendingPresentedCapture::Armed(capture) = pending_capture else {
        let result = window.compositor.present(
            &mut window.renderer,
            &mut window.surface,
            &window.viewport,
            background_color,
            on_pre_present,
        );
        return IcedPresentOutcome {
            result: finish_iced_present(
                result,
                render_candidate,
                precomputed_present_index,
                backend_present_index,
                last_successfully_presented,
            ),
            pending_capture: Some(pending_capture),
            capture_failure: None,
        };
    };

    use iced_renderer::fallback::{Compositor, Renderer, Surface};
    match (
        &mut window.compositor,
        &mut window.renderer,
        &mut window.surface,
    ) {
        (
            Compositor::Primary(compositor),
            Renderer::Primary(renderer),
            Surface::Primary(surface),
        ) => {
            let request = iced_renderer::wgpu::window::compositor::DirectCaptureRequest {
                token: capture.token,
                event_tx: capture.event_tx.clone(),
                wake: Arc::clone(&capture.wake),
            };
            let result = compositor.present_with_direct_capture(
                renderer,
                surface,
                &window.viewport,
                background_color,
                on_pre_present,
                request,
            );
            let result = finish_iced_present(
                result,
                render_candidate,
                precomputed_present_index,
                backend_present_index,
                last_successfully_presented,
            );
            if matches!(result, IcedPresentationResult::Presented) {
                IcedPresentOutcome {
                    result,
                    pending_capture: Some(PendingPresentedCapture::Waiting {
                        lease: capture,
                        committed_frame: last_successfully_presented
                            .as_ref()
                            .expect("successful direct present commits identity")
                            .clone(),
                        evidence: IcedCaptureEvidence::default(),
                    }),
                    capture_failure: None,
                }
            } else {
                IcedPresentOutcome {
                    result,
                    pending_capture: None,
                    capture_failure: Some((
                        capture,
                        "screenshot-surface-acquire-failed",
                        "the iced direct capture could not acquire the visible surface texture",
                    )),
                }
            }
        }
        (
            Compositor::Secondary(compositor),
            Renderer::Secondary(renderer),
            Surface::Secondary(surface),
        ) => {
            let result = compositor.present(
                renderer,
                surface,
                &window.viewport,
                background_color,
                on_pre_present,
            );
            IcedPresentOutcome {
                result: finish_iced_present(
                    result,
                    render_candidate,
                    precomputed_present_index,
                    backend_present_index,
                    last_successfully_presented,
                ),
                pending_capture: None,
                capture_failure: Some((
                    capture,
                    "screenshot-renderer-unsupported",
                    "the active iced tiny-skia fallback has no acquired GPU surface texture",
                )),
            }
        }
        _ => unreachable!("iced compositor, renderer, and surface variants must agree"),
    }
}

fn present_error_recovery(error: &compositor::SurfaceError) -> PresentErrorRecovery {
    match error {
        compositor::SurfaceError::OutOfMemory => PresentErrorRecovery::Exit,
        compositor::SurfaceError::Lost => PresentErrorRecovery::RecreateSurface,
        compositor::SurfaceError::Outdated => PresentErrorRecovery::ReconfigureSurface,
        compositor::SurfaceError::Timeout | compositor::SurfaceError::Other => {
            PresentErrorRecovery::RetryNextFrame
        }
    }
}

fn recover_window_from_present_error(
    window: &mut NativeIcedWindow,
    error: compositor::SurfaceError,
) -> PresentErrorRecovery {
    let recovery = present_error_recovery(&error);
    let PhysicalSize { width, height } = window.surface_physical_size;
    match recovery {
        PresentErrorRecovery::Exit => {
            eprintln!("[slipway-iced] unrecoverable surface present error: {error}");
        }
        PresentErrorRecovery::RecreateSurface => {
            eprintln!("[slipway-iced] surface present error: {error}; recreating the surface");
            window.surface = window
                .compositor
                .create_surface(window.raw.clone(), width, height);
            window.raw.request_redraw();
        }
        PresentErrorRecovery::ReconfigureSurface => {
            eprintln!("[slipway-iced] surface present error: {error}; reconfiguring the surface");
            window
                .compositor
                .configure_surface(&mut window.surface, width, height);
            window.raw.request_redraw();
        }
        PresentErrorRecovery::RetryNextFrame => {
            eprintln!("[slipway-iced] surface present error: {error}; retrying next frame");
            window.raw.request_redraw();
        }
    }
    recovery
}

fn apply_window_mouse_interaction(window: &mut NativeIcedWindow, interaction: mouse::Interaction) {
    if interaction == window.mouse_interaction {
        return;
    }
    if let Some(icon) = iced_winit::conversion::mouse_interaction(interaction) {
        window.raw.set_cursor_visible(true);
        window.raw.set_cursor(icon);
    } else {
        window.raw.set_cursor_visible(false);
    }
    window.mouse_interaction = interaction;
}

fn push_coalesced_pending_iced_event(
    events: &mut Vec<QueuedIcedIngress>,
    queued: QueuedIcedIngress,
) -> bool {
    let same_origin = events
        .last()
        .is_some_and(|last| iced_ingress_origins_can_coalesce(&last.origin, &queued.origin))
        && queued.commit_mutation_gate.is_none()
        && events
            .last()
            .is_some_and(|last| last.commit_mutation_gate.is_none());
    if same_origin
        && is_iced_cursor_moved_event(&queued.event)
        && let Some(last) = events.last_mut()
        && is_iced_cursor_moved_event(&last.event)
    {
        last.event = queued.event;
        merge_iced_ingress_completion(&mut last.origin, &queued.origin);
        true
    } else if same_origin
        && let iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) = &queued.event
        && let Some(QueuedIcedIngress {
            event: iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta: last_delta }),
            ..
        }) = events.last_mut()
        && let Some(summed) = summed_same_variant_scroll_delta(*last_delta, *delta)
    {
        *last_delta = summed;
        merge_iced_ingress_completion(
            &mut events.last_mut().expect("coalesced wheel exists").origin,
            &queued.origin,
        );
        true
    } else {
        events.push(queued);
        false
    }
}

fn iced_ingress_origins_can_coalesce(
    previous: &IcedIngressOrigin,
    next: &IcedIngressOrigin,
) -> bool {
    match (previous, next) {
        (IcedIngressOrigin::NativeOs, IcedIngressOrigin::NativeOs) => true,
        (
            IcedIngressOrigin::Debug {
                token: previous, ..
            },
            IcedIngressOrigin::Debug { token: next, .. },
        ) => previous == next,
        _ => false,
    }
}

fn merge_iced_ingress_completion(previous: &mut IcedIngressOrigin, next: &IcedIngressOrigin) {
    if let (
        IcedIngressOrigin::Debug {
            final_in_plan: previous,
            ..
        },
        IcedIngressOrigin::Debug {
            final_in_plan: next,
            ..
        },
    ) = (previous, next)
    {
        *previous |= *next;
    }
}

fn summed_same_variant_scroll_delta(
    previous: iced::mouse::ScrollDelta,
    next: iced::mouse::ScrollDelta,
) -> Option<iced::mouse::ScrollDelta> {
    match (previous, next) {
        (
            iced::mouse::ScrollDelta::Lines {
                x: previous_x,
                y: previous_y,
            },
            iced::mouse::ScrollDelta::Lines { x, y },
        ) => Some(iced::mouse::ScrollDelta::Lines {
            x: previous_x + x,
            y: previous_y + y,
        }),
        (
            iced::mouse::ScrollDelta::Pixels {
                x: previous_x,
                y: previous_y,
            },
            iced::mouse::ScrollDelta::Pixels { x, y },
        ) => Some(iced::mouse::ScrollDelta::Pixels {
            x: previous_x + x,
            y: previous_y + y,
        }),
        _ => None,
    }
}

fn is_iced_cursor_moved_event(event: &iced::Event) -> bool {
    matches!(
        event,
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { .. })
    )
}

fn pending_iced_events_are_cursor_only(events: &[QueuedIcedIngress]) -> bool {
    !events.is_empty()
        && events
            .iter()
            .all(|queued| is_iced_cursor_moved_event(&queued.event))
}

fn should_defer_pending_cursor_only(
    events: &[QueuedIcedIngress],
    _pressed_mouse_buttons: u8,
) -> bool {
    pending_iced_events_are_cursor_only(events)
}

fn debug_pointer_button(button: DebugPointerButton) -> MouseButton {
    match button {
        DebugPointerButton::Primary => MouseButton::Left,
        DebugPointerButton::Secondary => MouseButton::Right,
        DebugPointerButton::Auxiliary => MouseButton::Middle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slipway_debug_bridge::DebugPhysicalControlDeclarationSelector;

    fn native_ingress(event: iced::Event) -> QueuedIcedIngress {
        QueuedIcedIngress {
            event,
            origin: IcedIngressOrigin::NativeOs,
            commit_mutation_gate: None,
        }
    }

    fn debug_ingress(token: u64, final_in_plan: bool, event: iced::Event) -> QueuedIcedIngress {
        QueuedIcedIngress {
            event,
            origin: IcedIngressOrigin::Debug {
                token,
                final_in_plan,
            },
            commit_mutation_gate: None,
        }
    }

    fn cursor_moved_event(x: f32, y: f32) -> QueuedIcedIngress {
        native_ingress(iced::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: iced::Point::new(x, y),
        }))
    }

    fn wheel_lines_event(x: f32, y: f32) -> QueuedIcedIngress {
        native_ingress(iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Lines { x, y },
        }))
    }

    fn wheel_pixels_event(x: f32, y: f32) -> QueuedIcedIngress {
        native_ingress(iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x, y },
        }))
    }

    #[test]
    fn direct_capture_success_path_uses_only_the_primary_acquired_surface_hook() {
        let source = include_str!("native_runner.rs");
        let start = source
            .find("fn present_iced_window(")
            .expect("direct presentation helper exists");
        let end = source[start..]
            .find("fn present_error_recovery(")
            .map(|offset| start + offset)
            .expect("direct presentation helper remains bounded");
        let helper = &source[start..end];

        assert!(helper.contains("Compositor::Primary(compositor)"));
        assert!(helper.contains("present_with_direct_capture"));
        assert!(helper.contains("PendingPresentedCapture::Waiting"));
        assert!(helper.contains("Compositor::Secondary(compositor)"));
        assert!(helper.contains("screenshot-renderer-unsupported"));
        for forbidden in [
            "canonical",
            "offscreen",
            "screenshot(",
            "CaptureState",
            "unsafe",
        ] {
            assert!(
                !helper.contains(forbidden),
                "forbidden direct-capture fallback: {forbidden}"
            );
        }
    }

    #[test]
    fn direct_capture_refusal_codes_are_concrete_and_exhaustive() {
        use iced_renderer::wgpu::window::compositor::DirectCaptureRefusal::*;
        let refusals = [
            (CopySrcUnsupported, "screenshot-copy-src-unsupported"),
            (FormatUnsupported, "screenshot-format-unsupported"),
            (AlphaUnsupported, "screenshot-alpha-unsupported"),
            (ZeroSize, "screenshot-zero-size"),
            (ViewportUnavailable, "screenshot-viewport-unavailable"),
            (AlreadyArmed, "screenshot-already-armed"),
        ];
        for (refusal, expected) in refusals {
            let (code, reason) = direct_capture_refusal(refusal);
            assert_eq!(code, expected);
            assert!(!reason.is_empty());
        }
    }

    fn step223_capture_frame(index: u64) -> FrameIdentity {
        FrameIdentity {
            surface_id: "iced-step223-capture".to_string(),
            surface_instance_id: "iced-step223-instance".to_string(),
            revision: 7,
            frame_index: index,
            viewport: slipway_core::Rect {
                origin: slipway_core::Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 1.0,
                    height: 1.0,
                },
            },
        }
    }

    #[test]
    fn current_two_cycle_continuous_repaint_commits_distinct_actual_presentations() {
        let request_context = step223_capture_frame(0);
        let selectors = [
            PresentedScreenshotSelector::Current {
                request_context: request_context.clone(),
            },
            PresentedScreenshotSelector::Current {
                request_context: request_context.clone(),
            },
        ];
        let unchanged_render_candidate = step223_capture_frame(0);
        let mut backend_present_index = 0;
        let mut last_successfully_presented = None;
        let mut committed = Vec::new();

        for selector in selectors {
            let PresentedScreenshotSelector::Current { request_context } = selector else {
                panic!("fixture must retain Current custody");
            };
            let next = preflight_next_iced_present_index(backend_present_index)
                .expect("next repaint index is representable");
            commit_iced_successful_present(
                unchanged_render_candidate.clone(),
                next,
                &mut backend_present_index,
                &mut last_successfully_presented,
            );
            let presented = last_successfully_presented
                .as_ref()
                .expect("successful repaint commits identity");
            assert_ne!(presented.frame_index, request_context.frame_index);
            committed.push(presented.frame_index);
        }

        assert_eq!(committed, [1, 2]);
        assert_eq!(backend_present_index, 2);
    }

    #[test]
    fn present_index_overflow_preflight_reverts_and_fails_without_identity_mutation() {
        let mut backend_present_index = u64::MAX;
        let mut last_successfully_presented = Some(step223_capture_frame(u64::MAX));
        let before = last_successfully_presented.clone();

        assert_eq!(
            preflight_next_iced_present_index(backend_present_index),
            Err(IcedPresentIdentityOverflow)
        );
        assert_eq!(backend_present_index, u64::MAX);
        assert_eq!(last_successfully_presented, before);

        let source = include_str!("native_runner.rs");
        let start = source.find("fn present_iced_window(").unwrap();
        let end = source[start..]
            .find("fn present_error_recovery(")
            .map(|offset| start + offset)
            .unwrap();
        let helper = &source[start..end];
        let preflight = helper.find("preflight_next_iced_present_index").unwrap();
        let ordinary = helper.find("window.compositor.present(").unwrap();
        let direct = helper.find("present_with_direct_capture").unwrap();
        assert!(preflight < ordinary);
        assert!(preflight < direct);

        // Keep the bindings mutable to prove the failed preflight itself did not
        // consume or replace either owner.
        backend_present_index = 0;
        last_successfully_presented = None;
        assert_eq!(backend_present_index, 0);
        assert!(last_successfully_presented.is_none());
    }

    #[test]
    fn exact_next_requires_all_non_index_candidate_fields_unchanged() {
        let expected = step223_capture_frame(20);
        let mut captured = step223_capture_frame(21);
        assert!(iced_capture_frame_is_exact_next(&expected, &captured));
        captured.revision += 1;
        assert!(!iced_capture_frame_is_exact_next(&expected, &captured));
    }

    fn step223_presented_event(
        token: u64,
        width: u32,
    ) -> iced_renderer::wgpu::window::compositor::DirectCaptureEvent {
        use iced_renderer::wgpu::window::compositor::{
            DirectCaptureAlphaMode, DirectCaptureEvent, DirectCaptureFormat,
        };
        DirectCaptureEvent::Presented {
            token,
            format: DirectCaptureFormat::Rgba8Unorm,
            alpha: DirectCaptureAlphaMode::Opaque,
            width,
            height: 1,
        }
    }

    fn step223_mapped_event(
        token: u64,
        bytes: &[u8],
    ) -> iced_renderer::wgpu::window::compositor::DirectCaptureEvent {
        iced_renderer::wgpu::window::compositor::DirectCaptureEvent::Mapped {
            token,
            result: Ok(Arc::from(bytes)),
        }
    }

    fn assert_step223_capture_continues(reduction: IcedCaptureReduction) {
        assert!(matches!(reduction, IcedCaptureReduction::Continue));
    }

    #[test]
    fn step223_capture_state_accepts_either_event_order_and_retains_first_duplicates() {
        for mapped_first in [false, true] {
            let committed = step223_capture_frame(11);
            let mut evidence = IcedCaptureEvidence::default();
            if mapped_first {
                assert_step223_capture_continues(reduce_iced_capture_event(
                    41,
                    Some(&committed),
                    &mut evidence,
                    step223_mapped_event(41, &[1, 2, 3, 4]),
                ));
            }
            assert_step223_capture_continues(reduce_iced_capture_event(
                41,
                Some(&committed),
                &mut evidence,
                step223_presented_event(41, 1),
            ));
            assert_step223_capture_continues(reduce_iced_capture_event(
                41,
                Some(&committed),
                &mut evidence,
                step223_presented_event(41, 99),
            ));
            if !mapped_first {
                assert_step223_capture_continues(reduce_iced_capture_event(
                    41,
                    Some(&committed),
                    &mut evidence,
                    step223_mapped_event(41, &[1, 2, 3, 4]),
                ));
            }
            assert_step223_capture_continues(reduce_iced_capture_event(
                41,
                Some(&committed),
                &mut evidence,
                step223_mapped_event(41, &[9, 9, 9, 9]),
            ));

            let presented = evidence.presented.as_ref().expect("Presented retained");
            assert_eq!(presented.width, 1);
            assert_eq!(committed, step223_capture_frame(11));
            assert_eq!(evidence.mapped.as_deref(), Some(&[1, 2, 3, 4][..]));
        }
    }

    #[test]
    fn step223_capture_state_ignores_wrong_and_late_tokens_without_mutation() {
        let requested = step223_capture_frame(3);
        let mut evidence = IcedCaptureEvidence::default();
        assert_step223_capture_continues(reduce_iced_capture_event(
            52,
            Some(&requested),
            &mut evidence,
            step223_presented_event(51, 7),
        ));
        assert_step223_capture_continues(reduce_iced_capture_event(
            52,
            Some(&requested),
            &mut evidence,
            step223_mapped_event(51, &[7, 7, 7, 7]),
        ));
        assert!(evidence.presented.is_none());
        assert!(evidence.mapped.is_none());

        let committed = step223_capture_frame(4);
        assert_step223_capture_continues(reduce_iced_capture_event(
            52,
            Some(&committed),
            &mut evidence,
            step223_presented_event(52, 1),
        ));
        let captured = committed.clone();
        assert_step223_capture_continues(reduce_iced_capture_event(
            52,
            Some(&requested),
            &mut evidence,
            step223_presented_event(51, 88),
        ));
        assert_eq!(committed, captured);

        let mut next_request = IcedCaptureEvidence::default();
        assert_step223_capture_continues(reduce_iced_capture_event(
            53,
            Some(&requested),
            &mut next_request,
            step223_mapped_event(52, &[1, 2, 3, 4]),
        ));
        assert!(next_request.mapped.is_none());
    }

    #[test]
    fn step223_capture_state_refuses_map_poll_and_renderer_failures_once() {
        let requested = step223_capture_frame(4);
        for (signal, expected_code) in [
            (IcedCaptureSignal::MapFailed, "screenshot-map-failed"),
            (IcedCaptureSignal::PollFailed, "screenshot-poll-failed"),
            (
                IcedCaptureSignal::Refused {
                    code: "screenshot-zero-size",
                    reason: "zero size",
                },
                "screenshot-zero-size",
            ),
        ] {
            let mut evidence = IcedCaptureEvidence::default();
            let IcedCaptureReduction::Refused { code, .. } =
                reduce_iced_capture_signal(Some(&requested), &mut evidence, signal)
            else {
                panic!("failure must terminally refuse");
            };
            assert_eq!(code, expected_code);
            assert!(evidence.mapped.is_none());
        }

        let mut evidence = IcedCaptureEvidence::default();
        assert_step223_capture_continues(reduce_iced_capture_signal(
            Some(&requested),
            &mut evidence,
            IcedCaptureSignal::Mapped(Arc::from([1, 2, 3, 4])),
        ));
        assert_step223_capture_continues(reduce_iced_capture_signal(
            Some(&requested),
            &mut evidence,
            IcedCaptureSignal::MapFailed,
        ));
        assert_eq!(evidence.mapped.as_deref(), Some(&[1, 2, 3, 4][..]));
    }

    #[test]
    fn step223_capture_state_deadline_and_teardown_preserve_only_presented_identity() {
        let (captured, deadline_code, _) = iced_capture_deadline_refusal(None);
        assert!(captured.is_none());
        assert_eq!(deadline_code, "screenshot-deadline");
        let (captured, teardown_code, _) = iced_capture_teardown_refusal(None);
        assert!(captured.is_none());
        assert_eq!(teardown_code, "screenshot-window-teardown");

        let committed = step223_capture_frame(9);
        let mut waiting = IcedCaptureEvidence::default();
        assert_step223_capture_continues(reduce_iced_capture_event(
            61,
            Some(&committed),
            &mut waiting,
            step223_presented_event(61, 1),
        ));
        let expected = Some(step223_capture_frame(9));
        assert_eq!(iced_capture_deadline_refusal(Some(&committed)).0, expected);
        assert_eq!(iced_capture_teardown_refusal(Some(&committed)).0, expected);
    }

    struct Step223NoopCaptureWake;

    impl iced_renderer::wgpu::window::compositor::DirectCaptureWake for Step223NoopCaptureWake {
        fn wake(&self, _token: u64) {}
    }

    fn step223_frame_json(frame: &FrameIdentity) -> String {
        format!(
            r#"{{"surface_id":"{}","surface_instance_id":"{}","revision":{},"frame_index":{},"viewport":{{"origin":{{"x":{},"y":{}}},"size":{{"width":{},"height":{}}}}}}}"#,
            frame.surface_id,
            frame.surface_instance_id,
            frame.revision,
            frame.frame_index,
            frame.viewport.origin.x,
            frame.viewport.origin.y,
            frame.viewport.size.width,
            frame.viewport.size.height,
        )
    }

    #[test]
    fn step223_capture_state_deadline_spawn_failure_completes_real_lease_once() {
        let mut app = SlipwayIcedRuntimeApp::from_parts(crate::tests::TestWidget, (), |_, _| {});
        let requested_frame = app.runtime().last_frame_identity().clone();
        let request = format!(
            r#"{{"jsonrpc":"2.0","id":"iced-step223-spawn","method":"tools/call","params":{{"name":"slipway.debug.screenshot","arguments":{{"frame":{}}}}}}}"#,
            step223_frame_json(&requested_frame),
        );
        let response = app
            .debug_mcp()
            .submit_runtime_request(request)
            .expect("submit real screenshot MCP request");
        let pending = app
            .runtime_mut()
            .take_pending_native_mcp_call()
            .expect("take pending MCP call")
            .expect("screenshot request is pending");
        app.runtime_mut().record_presenting_backend(ICED_BACKEND_ID);
        let lease = app
            .runtime_mut()
            .take_debug_command_lease()
            .expect("take debug command lease")
            .expect("screenshot lease exists");
        let command = lease.command().clone();
        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(3);
        let wake: Arc<dyn iced_renderer::wgpu::window::compositor::DirectCaptureWake> =
            Arc::new(Step223NoopCaptureWake);

        let result = build_iced_capture_lease(
            71,
            command,
            lease,
            pending,
            event_rx,
            event_tx,
            wake,
            Duration::from_secs(5),
            |_| {
                Err(std::io::Error::other(
                    "deterministic Step 223 deadline spawn failure",
                ))
            },
        );
        let Err((command, lease, pending)) = result else {
            panic!("injected spawn failure must not arm pending capture state");
        };
        complete_presented_capture_refusal(
            command,
            lease,
            pending,
            None,
            "screenshot-deadline-thread-failed",
            "the iced direct capture deadline thread could not be created",
        );

        let first = response.try_recv().expect("the one response is delivered");
        let completion_count = usize::from(first.is_some());
        assert!(first.flatten().is_some_and(|reply| {
            reply
                .to_string()
                .contains("screenshot-deadline-thread-failed")
        }));
        assert!(
            !matches!(response.try_recv(), Ok(Some(_))),
            "the real pending response must complete exactly once"
        );
        assert_eq!(completion_count, 1);
    }

    #[test]
    fn direct_capture_revert_guard_requires_surface_configuration_restore() {
        let source = include_str!("../../../third_party/iced_wgpu-0.14.0/src/window/compositor.rs");
        let start = source
            .find("pub fn present_with_direct_capture")
            .expect("fork direct hook exists");
        let end = source[start..]
            .find("fn surface_configuration(")
            .map(|offset| start + offset)
            .expect("fork hook remains bounded");
        let hook = &source[start..end];
        assert!(hook.contains("renderer.present"));
        assert!(hook.contains("encoder.copy_texture_to_buffer"));
        assert!(hook.contains("frame.present()"));
        assert!(hook.contains("restore_surface_configuration"));
        let copy = hook
            .find("encoder.copy_texture_to_buffer")
            .expect("direct texture copy exists");
        let present = hook[copy..]
            .find("frame.present()")
            .map(|offset| copy + offset)
            .expect("the copied acquired texture is subsequently presented");
        let restore = hook[present..]
            .find("restore_surface_configuration")
            .map(|offset| present + offset)
            .expect("request-only surface configuration is reverted");
        assert!(copy < present);
        assert!(present < restore);
    }

    #[test]
    fn admitted_composition_commit_gate_moves_once_without_coalescing() {
        let gate = IcedNativeCommitMutationGate {
            token: 31,
            commit_sequence_index: 2,
            target: WidgetId::from("text-target"),
            target_slot: None,
            region_id: PresentationRegionId::from("text-region"),
        };
        let mut pending = Vec::new();
        assert!(!push_coalesced_pending_iced_event(
            &mut pending,
            QueuedIcedIngress {
                event: iced::Event::InputMethod(iced_core::input_method::Event::Commit(
                    "commit".to_string(),
                )),
                origin: IcedIngressOrigin::DebugComposition {
                    token: 31,
                    sequence_index: 2,
                    phase: TextCompositionPhase::Commit,
                },
                commit_mutation_gate: Some(gate),
            },
        ));
        assert!(!push_coalesced_pending_iced_event(
            &mut pending,
            debug_ingress(31, true, cursor_moved_event(1.0, 1.0).event),
        ));

        let slices = pending_dispatch_slices(pending);
        assert_eq!(slices.len(), 2);
        let moved_gate = slices[0]
            .commit_mutation_gate
            .as_ref()
            .expect("the expected Commit owns its one non-cloned gate");
        assert_eq!(moved_gate.token, 31);
        assert_eq!(moved_gate.commit_sequence_index, 2);
        assert!(slices[1].commit_mutation_gate.is_none());
    }

    #[test]
    fn composition_unicode_scalar_ranges_convert_to_winit_byte_ranges() {
        let text = "a\u{1F642}\u{D55C}";
        assert_eq!(unicode_scalar_to_byte_index(text, 0), 0);
        assert_eq!(unicode_scalar_to_byte_index(text, 1), 1);
        assert_eq!(unicode_scalar_to_byte_index(text, 2), 5);
        assert_eq!(unicode_scalar_to_byte_index(text, 3), text.len());
        assert_eq!(unicode_scalar_to_byte_index(text, 99), text.len());
    }

    #[test]
    fn iced_native_physical_text_uses_ime_commit_event() {
        let events = iced_events_for_native_physical_operation(
            &DebugPhysicalControl::Text {
                selector: DebugPhysicalControlDeclarationSelector::Target {
                    target: WidgetId::from("text-target"),
                },
                text: "abc".to_string(),
            },
            1.0,
            ModifiersState::default(),
        )
        .expect("text physical control maps to iced event");

        let [iced::Event::InputMethod(event)] = events.as_slice() else {
            panic!("expected a single input method commit event");
        };
        assert_eq!(format!("{event:?}"), r#"Commit("abc")"#);
    }

    #[test]
    fn iced_native_physical_keyboard_uses_iced_keyboard_event() {
        let events = iced_events_for_native_physical_operation(
            &DebugPhysicalControl::Keyboard {
                selector: DebugPhysicalControlDeclarationSelector::Target {
                    target: WidgetId::from("text-target"),
                },
                key: "Enter".to_string(),
                kind: slipway_core::KeyEventKind::Press,
                modifiers: slipway_core::Modifiers::default(),
                details: slipway_core::KeyboardDetails::default(),
            },
            1.0,
            ModifiersState::default(),
        )
        .expect("keyboard physical control maps to iced event");

        assert!(matches!(
            events.as_slice(),
            [iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
                ..
            })]
        ));
    }

    fn pointer_operation(kind: slipway_core::PointerEventKind) -> DebugPhysicalControl {
        DebugPhysicalControl::Pointer {
            position: Point { x: 4.0, y: 4.0 },
            kind,
            button: Some(DebugPointerButton::Primary),
            details: slipway_core::PointerDetails::default(),
            pointer_is_pressed: matches!(kind, slipway_core::PointerEventKind::Press),
        }
    }

    #[test]
    fn seam_pointer_press_and_release_mirror_pressed_mouse_button_bookkeeping() {
        // MF-M18: the seam must leave the same `pressed_mouse_buttons`
        // side-state the real WindowEvent::MouseInput path leaves.
        let pressed = seam_pressed_mouse_buttons_after(
            0,
            &pointer_operation(slipway_core::PointerEventKind::Press),
        );
        assert_eq!(pressed, 1);
        let released = seam_pressed_mouse_buttons_after(
            pressed,
            &pointer_operation(slipway_core::PointerEventKind::Release),
        );
        assert_eq!(released, 0);
        // Saturates like the real path instead of underflowing.
        assert_eq!(
            seam_pressed_mouse_buttons_after(
                0,
                &pointer_operation(slipway_core::PointerEventKind::Release)
            ),
            0
        );
        // Moves and non-pointer operations leave the count alone.
        assert_eq!(
            seam_pressed_mouse_buttons_after(
                1,
                &pointer_operation(slipway_core::PointerEventKind::Move)
            ),
            1
        );
        assert_eq!(
            seam_pressed_mouse_buttons_after(
                1,
                &DebugPhysicalControl::Wheel {
                    position: Point { x: 4.0, y: 4.0 },
                    delta_x: 0.0,
                    delta_y: 1.0,
                }
            ),
            1
        );
    }

    #[test]
    fn seam_keyboard_modifiers_map_to_winit_modifiers_state() {
        // MF-M18: seam keyboard operations mirror the real path's
        // ModifiersChanged side-state.
        assert_eq!(
            winit_modifiers_state_from_slipway(slipway_core::Modifiers::default()),
            ModifiersState::empty()
        );
        let full = winit_modifiers_state_from_slipway(slipway_core::Modifiers {
            shift: true,
            control: true,
            alt: true,
            meta: true,
        });
        assert_eq!(
            full,
            ModifiersState::SHIFT
                | ModifiersState::CONTROL
                | ModifiersState::ALT
                | ModifiersState::SUPER
        );
        assert_eq!(
            winit_modifiers_state_from_slipway(slipway_core::Modifiers {
                shift: false,
                control: true,
                alt: false,
                meta: false,
            }),
            ModifiersState::CONTROL
        );
    }

    #[test]
    fn seam_no_match_failure_states_events_were_dispatched() {
        // MF-M18: the no-match refusal must admit the half-applied reality
        // instead of implying nothing happened.
        let failure = native_physical_no_match_failure(None, 2);
        assert_eq!(
            failure.code,
            "native-physical-control-produced-no-backend-input"
        );
        assert!(
            failure.message.contains("events were dispatched"),
            "{}",
            failure.message
        );
        assert!(
            failure
                .message
                .contains("2 non-matching backend input message(s) were withheld"),
            "{}",
            failure.message
        );
        assert!(failure.dispatch_evidence.is_none());
    }

    #[test]
    fn seam_no_match_failure_attaches_post_hoc_diagnosis() {
        let refusal = slipway_core::DeclaredEventDispatchEvidence {
            source: slipway_core::EvidenceSource::backend_presented(
                ICED_BACKEND_ID,
                "physical-input",
            ),
            frame: FrameIdentity {
                surface_id: "test-surface".to_string(),
                surface_instance_id: "test-instance".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 100.0,
                    },
                },
            },
            kind: DeclaredEventDispatchKind::Wheel,
            input_position: Some(Point { x: 4.0, y: 4.0 }),
            input_position_space: Some(slipway_core::DispatchPositionSpace::Content),
            candidate_regions: vec![PresentationRegionId::from("dead-region")],
            selected_region: None,
            refusal_reason: Some("wheel found no scrollable consumer".to_string()),
            generated_event: None,
            route: None,
            capture_event: false,
            diagnostics: Vec::new(),
        };

        let failure = native_physical_no_match_failure(Some(refusal), 0);

        let diagnosis = failure
            .dispatch_evidence
            .as_ref()
            .expect("no-match failure attaches the post-hoc diagnosis");
        assert_eq!(
            diagnosis.source.label(),
            slipway_core::EVIDENCE_SOURCE_POST_HOC_DIAGNOSIS
        );
        assert!(
            failure.message.contains("events were dispatched"),
            "{}",
            failure.message
        );
        assert!(
            failure
                .message
                .contains("wheel found no scrollable consumer"),
            "{}",
            failure.message
        );
        assert!(
            failure.message.contains("dead-region"),
            "{}",
            failure.message
        );
    }

    #[test]
    fn iced_native_physical_text_edit_delete_uses_keyboard_event() {
        let events = iced_events_for_native_physical_operation(
            &DebugPhysicalControl::TextEdit {
                selector: DebugPhysicalControlDeclarationSelector::Target {
                    target: WidgetId::from("text-target"),
                },
                kind: slipway_core::TextEditKind::DeleteBackward,
                text: None,
                selection_before: None,
                selection_after: None,
            },
            1.0,
            ModifiersState::default(),
        )
        .expect("delete text edit maps to native backspace");

        assert!(matches!(
            events.as_slice(),
            [iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace),
                ..
            })]
        ));
    }

    #[test]
    fn pending_iced_events_coalesce_adjacent_cursor_moves() {
        let mut events = Vec::new();

        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            cursor_moved_event(10.0, 20.0),
        ));
        assert!(push_coalesced_pending_iced_event(
            &mut events,
            cursor_moved_event(30.0, 40.0),
        ));

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].event,
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position })
                if *position == iced::Point::new(30.0, 40.0)
        ));
    }

    #[test]
    fn pending_iced_events_preserve_button_boundaries_between_cursor_moves() {
        let mut events = Vec::new();

        push_coalesced_pending_iced_event(&mut events, cursor_moved_event(10.0, 20.0));
        push_coalesced_pending_iced_event(
            &mut events,
            native_ingress(iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            ))),
        );
        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            cursor_moved_event(30.0, 40.0),
        ));

        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[0].event,
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position })
                if *position == iced::Point::new(10.0, 20.0)
        ));
        assert!(matches!(
            &events[1].event,
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
        ));
        assert!(matches!(
            &events[2].event,
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position })
                if *position == iced::Point::new(30.0, 40.0)
        ));
    }

    #[test]
    fn pending_iced_events_preserve_wheel_boundary_after_cursor_move() {
        let mut events = Vec::new();

        push_coalesced_pending_iced_event(&mut events, cursor_moved_event(10.0, 20.0));
        push_coalesced_pending_iced_event(
            &mut events,
            native_ingress(iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            })),
        );

        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].event,
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position })
                if *position == iced::Point::new(10.0, 20.0)
        ));
        assert!(matches!(
            &events[1].event,
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled { .. })
        ));
    }

    #[test]
    fn pending_iced_events_coalesce_adjacent_same_variant_wheel_events_by_summation() {
        let mut events = Vec::new();

        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            wheel_lines_event(1.0, -1.0),
        ));
        assert!(push_coalesced_pending_iced_event(
            &mut events,
            wheel_lines_event(0.5, -2.0),
        ));

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].event,
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x, y },
            }) if *x == 1.5 && *y == -3.0
        ));

        let mut events = Vec::new();

        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            wheel_pixels_event(2.0, -24.0),
        ));
        assert!(push_coalesced_pending_iced_event(
            &mut events,
            wheel_pixels_event(-1.0, -16.0),
        ));

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].event,
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Pixels { x, y },
            }) if *x == 1.0 && *y == -40.0
        ));
    }

    #[test]
    fn pending_iced_events_never_merge_lines_and_pixels_wheel_deltas() {
        let mut events = Vec::new();

        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            wheel_lines_event(0.0, -1.0),
        ));
        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            wheel_pixels_event(0.0, -24.0),
        ));
        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            wheel_lines_event(0.0, -2.0),
        ));

        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[0].event,
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x, y },
            }) if *x == 0.0 && *y == -1.0
        ));
        assert!(matches!(
            &events[1].event,
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Pixels { x, y },
            }) if *x == 0.0 && *y == -24.0
        ));
        assert!(matches!(
            &events[2].event,
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x, y },
            }) if *x == 0.0 && *y == -2.0
        ));
    }

    #[test]
    fn pending_iced_events_never_merge_wheel_events_across_cursor_move_boundary() {
        let mut events = Vec::new();

        push_coalesced_pending_iced_event(&mut events, wheel_lines_event(0.0, -1.0));
        push_coalesced_pending_iced_event(&mut events, cursor_moved_event(10.0, 20.0));
        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            wheel_lines_event(0.0, -2.0),
        ));

        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[0].event,
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x, y },
            }) if *x == 0.0 && *y == -1.0
        ));
        assert!(matches!(
            &events[1].event,
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position })
                if *position == iced::Point::new(10.0, 20.0)
        ));
        assert!(matches!(
            &events[2].event,
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x, y },
            }) if *x == 0.0 && *y == -2.0
        ));
    }

    #[test]
    fn pending_wheel_events_flush_at_the_current_iteration_boundary_not_the_next_redraw() {
        let mut events = Vec::new();
        push_coalesced_pending_iced_event(&mut events, wheel_lines_event(0.0, -1.0));

        assert!(
            !should_defer_pending_cursor_only(&events, 0),
            "a queued wheel event must dispatch in the same about_to_wait cycle instead of \
             deferring to the next redraw like the cursor-only storm path"
        );

        let mut events = Vec::new();
        push_coalesced_pending_iced_event(&mut events, cursor_moved_event(10.0, 20.0));
        assert!(should_defer_pending_cursor_only(&events, 0));
        push_coalesced_pending_iced_event(&mut events, wheel_lines_event(0.0, -1.0));

        assert!(
            !should_defer_pending_cursor_only(&events, 0),
            "a mixed [cursor-move..., wheel] queue must not be treated as cursor-only \
             deferrable; it must dispatch at the current about_to_wait boundary"
        );
    }

    #[test]
    fn pending_iced_events_cursor_only_requires_deferred_frame_flush() {
        let mut events = Vec::new();

        assert!(!pending_iced_events_are_cursor_only(&events));
        push_coalesced_pending_iced_event(&mut events, cursor_moved_event(10.0, 20.0));
        assert!(pending_iced_events_are_cursor_only(&events));
        push_coalesced_pending_iced_event(
            &mut events,
            native_ingress(iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            ))),
        );
        assert!(!pending_iced_events_are_cursor_only(&events));
    }

    #[test]
    fn pending_cursor_moves_defer_to_next_redraw_even_while_mouse_button_is_pressed() {
        let mut events = Vec::new();
        push_coalesced_pending_iced_event(&mut events, cursor_moved_event(10.0, 20.0));

        assert!(should_defer_pending_cursor_only(&events, 0));
        assert!(
            should_defer_pending_cursor_only(&events, 1),
            "drag cursor movement must be coalesced to the next redraw so raw input rate cannot force more than one drag update per visible frame"
        );
    }

    #[test]
    fn pending_dispatch_slices_flush_a_coalesced_wheel_burst_as_one_single_build_slice() {
        let mut pending = Vec::new();
        push_coalesced_pending_iced_event(&mut pending, wheel_lines_event(0.0, -1.0));
        push_coalesced_pending_iced_event(&mut pending, wheel_lines_event(0.0, -2.0));

        let slices = pending_dispatch_slices(pending);

        assert_eq!(
            slices.len(),
            1,
            "a coalesced wheel burst is a single queued event and must keep the \
             single-build flush fast path"
        );
        assert!(matches!(
            slices[0].events.as_slice(),
            [iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x, y },
            })] if *x == 0.0 && *y == -3.0
        ));
    }

    #[test]
    fn pending_dispatch_slices_never_share_a_build_between_a_wheel_and_a_later_press() {
        let mut pending = Vec::new();
        push_coalesced_pending_iced_event(&mut pending, wheel_lines_event(0.0, -1.0));
        push_coalesced_pending_iced_event(
            &mut pending,
            native_ingress(iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            ))),
        );

        let slices = pending_dispatch_slices(pending);

        assert_eq!(
            slices.len(),
            2,
            "a queued wheel must dispatch and apply its message before a later press is \
             built: sharing one build stamps the press evidence with the pre-wheel frame \
             revision, and the runtime refuses it as \
             BACKEND_INPUT_DISPATCH_EVIDENCE_FRAME_MISMATCH (silently dropped drag grab)"
        );
        assert!(matches!(
            slices[0].events.as_slice(),
            [iced::Event::Mouse(iced::mouse::Event::WheelScrolled { .. })]
        ));
        assert!(matches!(
            slices[1].events.as_slice(),
            [iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left
            ))]
        ));
    }

    #[test]
    fn pending_dispatch_slices_keep_coalesced_cursor_moves_as_one_single_build_slice() {
        let mut pending = Vec::new();
        push_coalesced_pending_iced_event(&mut pending, cursor_moved_event(10.0, 20.0));
        push_coalesced_pending_iced_event(&mut pending, cursor_moved_event(30.0, 40.0));

        let slices = pending_dispatch_slices(pending);

        assert_eq!(
            slices.len(),
            1,
            "coalesced cursor moves collapse to one queued event, so the cursor-storm \
             flush keeps its one-build behavior"
        );
        assert!(matches!(
            slices[0].events.as_slice(),
            [iced::Event::Mouse(iced::mouse::Event::CursorMoved { position })]
                if *position == iced::Point::new(30.0, 40.0)
        ));
    }

    #[test]
    fn pending_dispatch_slices_yield_one_slice_per_event_preserving_queue_order() {
        // Strict per-event slicing is intentional, not an implementation
        // detail: iced's `UserInterface::update` gathers messages across the
        // whole slice with no per-event attribution, so the flush cannot know
        // which event produced a message and therefore cannot batch "until
        // the first message-producing event". The only split that guarantees
        // every message is applied before the next event's dispatch evidence
        // is stamped is one slice per queued event, in queue order.
        let mut pending = Vec::new();
        push_coalesced_pending_iced_event(&mut pending, cursor_moved_event(10.0, 20.0));
        push_coalesced_pending_iced_event(
            &mut pending,
            native_ingress(iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            ))),
        );
        push_coalesced_pending_iced_event(&mut pending, wheel_lines_event(0.0, -1.0));
        push_coalesced_pending_iced_event(&mut pending, cursor_moved_event(30.0, 40.0));
        let queued = pending
            .iter()
            .map(|queued| format!("{:?}", queued.event))
            .collect::<Vec<_>>();

        let slices = pending_dispatch_slices(pending);

        assert_eq!(
            slices.len(),
            queued.len(),
            "one dispatch slice per queued event"
        );
        for (slice, event) in slices.iter().zip(queued.iter()) {
            assert_eq!(slice.events.len(), 1);
            assert_eq!(format!("{:?}", slice.events[0]), *event);
        }
    }

    #[test]
    fn pending_debug_events_coalesce_only_within_one_token_and_keep_final_marker() {
        let mut events = Vec::new();
        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            debug_ingress(7, false, cursor_moved_event(1.0, 2.0).event),
        ));
        assert!(push_coalesced_pending_iced_event(
            &mut events,
            debug_ingress(7, true, cursor_moved_event(3.0, 4.0).event),
        ));
        assert!(!push_coalesced_pending_iced_event(
            &mut events,
            debug_ingress(8, true, cursor_moved_event(5.0, 6.0).event),
        ));

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].origin,
            IcedIngressOrigin::Debug {
                token: 7,
                final_in_plan: true,
            }
        );
        assert_eq!(
            events[1].origin,
            IcedIngressOrigin::Debug {
                token: 8,
                final_in_plan: true,
            }
        );
    }

    #[test]
    fn redraw_update_re_ticks_only_for_message_free_layout_changes_with_reference_bound() {
        assert!(should_re_tick_redraw_update(true, true, 1));
        assert!(should_re_tick_redraw_update(true, true, 2));
        assert!(
            !should_re_tick_redraw_update(true, true, 3),
            "iced_winit caps consecutive redraw updates at three per frame"
        );
        assert!(
            !should_re_tick_redraw_update(false, true, 1),
            "messages break the re-tick loop and take the message-application flow"
        );
        assert!(!should_re_tick_redraw_update(true, false, 1));
    }

    #[test]
    fn redraw_request_honoring_matches_iced_winit_schedule_semantics() {
        let now = Instant::now();
        let earlier = now + Duration::from_millis(4);
        let later = now + Duration::from_millis(16);

        assert_eq!(
            redraw_request_action(iced::window::RedrawRequest::NextFrame, None),
            RedrawRequestAction::RequestNow
        );
        assert_eq!(
            redraw_request_action(iced::window::RedrawRequest::NextFrame, Some(later)),
            RedrawRequestAction::RequestNow
        );
        assert_eq!(
            redraw_request_action(iced::window::RedrawRequest::At(earlier), None),
            RedrawRequestAction::ScheduleAt(earlier)
        );
        assert_eq!(
            redraw_request_action(iced::window::RedrawRequest::At(earlier), Some(later)),
            RedrawRequestAction::ScheduleAt(earlier)
        );
        assert_eq!(
            redraw_request_action(iced::window::RedrawRequest::At(later), Some(earlier)),
            RedrawRequestAction::Keep,
            "a later At request must not push back an earlier scheduled wakeup"
        );
        assert_eq!(
            redraw_request_action(iced::window::RedrawRequest::Wait, None),
            RedrawRequestAction::Keep,
            "Wait must request nothing so redraw ticks cannot self-sustain a repaint storm"
        );
        assert_eq!(
            redraw_request_action(iced::window::RedrawRequest::Wait, Some(earlier)),
            RedrawRequestAction::Keep
        );
    }

    #[test]
    fn present_error_recovery_matches_the_iced_winit_reference_actions() {
        assert_eq!(
            present_error_recovery(&compositor::SurfaceError::OutOfMemory),
            PresentErrorRecovery::Exit
        );
        assert_eq!(
            present_error_recovery(&compositor::SurfaceError::Lost),
            PresentErrorRecovery::RecreateSurface
        );
        assert_eq!(
            present_error_recovery(&compositor::SurfaceError::Outdated),
            PresentErrorRecovery::ReconfigureSurface
        );
        assert_eq!(
            present_error_recovery(&compositor::SurfaceError::Timeout),
            PresentErrorRecovery::RetryNextFrame
        );
        assert_eq!(
            present_error_recovery(&compositor::SurfaceError::Other),
            PresentErrorRecovery::RetryNextFrame
        );
    }

    #[test]
    fn handled_input_without_state_or_message_change_does_not_request_redraw() {
        let update = SlipwayIcedRuntimeAppUpdate {
            runtime_update: Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 0,
                diagnostics: Vec::new(),
            }),
            debug_replies_drained: 0,
            debug_error: None,
        };

        assert!(!iced_runtime_update_requires_redraw(&update, false));
        assert!(iced_runtime_update_requires_redraw(&update, true));
    }

    #[test]
    fn input_redraw_gate_preserves_messages_diagnostics_and_debug_errors() {
        let with_message = SlipwayIcedRuntimeAppUpdate {
            runtime_update: Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            }),
            debug_replies_drained: 0,
            debug_error: None,
        };
        assert!(iced_runtime_update_requires_redraw(&with_message, false));

        let with_error = SlipwayIcedRuntimeAppUpdate {
            runtime_update: Some(SlipwayIcedRuntimeUpdate::Noop),
            debug_replies_drained: 0,
            debug_error: Some("debug failed".to_string()),
        };
        assert!(iced_runtime_update_requires_redraw(&with_error, false));
    }
}
