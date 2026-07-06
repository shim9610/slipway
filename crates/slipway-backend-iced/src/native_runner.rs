use super::*;

use iced::advanced::Renderer as _;
use iced::advanced::graphics::Compositor as _;
use iced::advanced::graphics::Viewport;
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Renderer as _};
use iced::theme::Base as _;
use iced_core::input_method;
use iced_winit::runtime::user_interface::{self, UserInterface};
use iced_winit::winit;
use slipway_core::{PointerButton as DebugPointerButton, Size};
use slipway_debug_bridge::{
    DebugPhysicalControl, VISIBLE_FRAME_BUDGET_NS, VisibleFrameTimingRecorder,
};
use slipway_runtime::{SlipwayImePolicy, SlipwayRuntimePendingNativeMcpCall};
use std::borrow::Cow;
use std::sync::Arc;
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
    pending_iced_events: Vec<iced::Event>,
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
        Self {
            app,
            display_handle,
            proxy,
            ime_policy,
            ime_trace,
            window: None,
            pending_mcp: None,
            pending_iced_events: Vec::new(),
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
        let scale_factor = 1.0;
        let title = self.app.title();
        let window_start = Instant::now();
        let window = Arc::new(
            event_loop
                .create_window(iced_winit::conversion::window_attributes(
                    window_settings,
                    &title,
                    scale_factor,
                    None,
                    None,
                ))
                .map_err(|error| iced::Error::WindowCreationFailed(Box::new(error)))?,
        );
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

    fn process_messages(&mut self, messages: Vec<SlipwayIcedRuntimeMessage>) {
        let mut should_redraw = false;
        for message in messages {
            if let SlipwayIcedRuntimeMessage::PreeditStyle(style) = message {
                self.apply_preedit_style(style);
                continue;
            }
            let update = self.app.update_without_debug_drain(message);
            should_redraw = true;
            if update.debug_error.is_some() {
                should_redraw = true;
            }
        }
        if should_redraw {
            if let Some(window) = self.window.as_ref() {
                window.raw.request_redraw();
            }
        }
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

    fn redraw(&mut self) {
        let timing_start = Instant::now();
        let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
        let cursor = self.cursor();
        let Some(window) = self.window.as_mut() else {
            return;
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
        let present_start = Instant::now();
        let present_result = window.compositor.present(
            &mut window.renderer,
            &mut window.surface,
            &window.viewport,
            base.background_color,
            || {},
        );
        let present_elapsed = present_start.elapsed();
        if present_result.is_ok() {
            window.presented_frames += 1;
        }
        let presented_frame_after = window.presented_frames;
        let _ = window;
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
                self.dispatch_pending_iced_events();
                if self
                    .window
                    .as_ref()
                    .is_some_and(should_dispatch_redraw_event_update)
                {
                    let messages = self.dispatch_iced_events(&[iced::Event::Window(
                        iced::window::Event::RedrawRequested(Instant::now()),
                    )]);
                    self.process_messages(messages);
                }
                self.redraw();
                let timing_viewport = self.window.as_ref().map(iced_timing_viewport);
                self.frame_timing.record(
                    "iced.redraw_requested",
                    timing_start.elapsed(),
                    pending_event_count,
                    timing_viewport,
                );
                return;
            }
            _ => {}
        }

        let Some(window) = self.window.as_ref() else {
            return;
        };
        if let Some(event) = iced_winit::conversion::window_event(
            event,
            window.viewport.scale_factor(),
            window.modifiers,
        ) {
            self.pending_iced_events.push(event);
            if self.pending_resize.is_none() {
                self.dispatch_pending_iced_events();
                if let Some(window) = self.window.as_ref() {
                    window.raw.request_redraw();
                }
            } else if self
                .window
                .as_ref()
                .is_some_and(|window| window.presented_frames == 0)
            {
                window.raw.request_redraw();
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
        self.pending_iced_events
            .push(iced::Event::Window(iced::window::Event::Resized(
                iced::Size::new(logical.width, logical.height),
            )));
    }

    fn dispatch_pending_iced_events(&mut self) {
        if self.pending_iced_events.is_empty() {
            return;
        }
        let events = std::mem::take(&mut self.pending_iced_events);
        let messages = self.dispatch_iced_events(&events);
        self.process_messages(messages);
    }

    fn pump_mcp(&mut self) {
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
        let lease = match self.app.runtime_mut().take_debug_command_lease() {
            Ok(Some(lease)) => lease,
            Ok(None) | Err(_) => {
                self.pending_mcp = Some(pending);
                return;
            }
        };
        let command = lease.command().clone();
        let DebugCommandKind::PhysicalControl { ref operation, .. } = command.kind else {
            let app = &mut self.app;
            let _ = app
                .assembled
                .runtime
                .complete_debug_command_lease_with_app_reducer(lease, &mut app.apply_app_messages);
            let _ = pending.try_finish_and_respond();
            return;
        };
        let product = self.run_native_physical_control(&command, operation);
        let _ = lease.complete(product);
        let _ = pending.try_finish_and_respond();
        if let Some(window) = self.window.as_ref() {
            window.raw.request_redraw();
        }
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
                selector,
                offset_x,
                offset_y,
            } => {
                return self.run_native_scroll_control(command, selector, *offset_x, *offset_y);
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
        let messages = self.dispatch_iced_events(&events);
        let mut backend_inputs = Vec::new();
        let mut remaining = Vec::new();
        for message in messages {
            match message {
                SlipwayIcedRuntimeMessage::BackendInput(input) => backend_inputs.push(input),
                other => remaining.push(other),
            }
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
                    return DebugReplyProduct::Error(DebugFailure {
                        code: "native-physical-control-produced-no-backend-input".to_string(),
                        message: "the synthesized iced native event reached UserInterface::update but produced no backend-presented input evidence matching the requested physical operation".to_string(),
                        dispatch_evidence: None,
                    });
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

    fn run_native_scroll_control(
        &mut self,
        command: &DebugCommand,
        selector: &slipway_debug_bridge::DebugPhysicalControlDeclarationSelector,
        offset_x: f32,
        offset_y: f32,
    ) -> DebugReplyProduct {
        let presentation = match self.current_visible_presentation() {
            Ok(presentation) => presentation,
            Err(unsupported) => return native_physical_control_error(unsupported),
        };
        let Some(region) =
            scroll_region_for_native_physical_selector(&presentation, selector).cloned()
        else {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-scroll-region-not-found",
                "the current iced visible presentation has no enabled scroll region matching the physical control selector",
            ));
        };
        if !region.axes.horizontal && offset_x != 0.0 {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-scroll-horizontal-disabled",
                "the selected iced scroll region does not declare horizontal scrolling",
            ));
        }
        if !region.axes.vertical && offset_y != 0.0 {
            return native_physical_control_error(NativePhysicalControlUnsupported::new(
                "native-physical-control-scroll-vertical-disabled",
                "the selected iced scroll region does not declare vertical scrolling",
            ));
        }
        let mut operation = iced::advanced::widget::operation::scrollable::scroll_to(
            iced_scrollable_id(&region),
            iced::advanced::widget::operation::scrollable::AbsoluteOffset {
                x: Some(offset_x.max(0.0)),
                y: Some(offset_y.max(0.0)),
            },
        );
        if let Err(unsupported) = self.operate_visible_ui(&mut operation) {
            return native_physical_control_error(unsupported);
        }
        let backend_input = backend_native_scroll_input_event(
            &presentation,
            &region,
            offset_x.max(0.0),
            offset_y.max(0.0),
        );
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
            _ => {}
        }
        iced_events_for_native_physical_operation(operation, scale_factor, window.modifiers)
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

fn native_physical_control_error(
    unsupported: NativePhysicalControlUnsupported,
) -> DebugReplyProduct {
    DebugReplyProduct::Error(DebugFailure {
        code: unsupported.code.to_string(),
        message: unsupported.message.to_string(),
        dispatch_evidence: None,
    })
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
        self.dispatch_pending_iced_events();
        let Some(window) = self.window.as_mut() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        if window.presented_frames == 0 {
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

fn request_window_redraw(window: &mut NativeIcedWindow, request: iced::window::RedrawRequest) {
    match request {
        iced::window::RedrawRequest::NextFrame => {
            window.redraw_at = None;
            window.raw.request_redraw();
        }
        iced::window::RedrawRequest::At(at) => {
            if window.redraw_at.is_none_or(|scheduled| at < scheduled) {
                window.redraw_at = Some(at);
            }
        }
        iced::window::RedrawRequest::Wait => {}
    }
}

fn should_dispatch_redraw_event_update(window: &NativeIcedWindow) -> bool {
    window.ime_composing || window.ime_state.is_some() || window.preedit.is_some()
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
}
