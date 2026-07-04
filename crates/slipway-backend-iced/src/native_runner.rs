use super::*;

use iced::advanced::graphics::Compositor as _;
use iced::advanced::graphics::Viewport;
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::theme::Base as _;
use iced_winit::runtime::user_interface::{self, UserInterface};
use iced_winit::winit;
use slipway_core::PointerButton as DebugPointerButton;
use slipway_debug_bridge::DebugPhysicalControl;
use slipway_runtime::SlipwayRuntimePendingNativeMcpCall;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{
    DeviceId, ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{
    ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle,
};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

#[derive(Clone, Copy, Debug)]
enum NativeRunnerEvent {
    McpWake,
}

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
    let mut runner = NativeIcedRunner::new(app, display_handle, proxy);

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
    window: Option<NativeIcedWindow>,
    pending_mcp: Option<SlipwayRuntimePendingNativeMcpCall>,
}

struct NativeIcedWindow {
    raw: Arc<Window>,
    compositor: iced_renderer::Compositor,
    surface: <iced_renderer::Compositor as iced::advanced::graphics::Compositor>::Surface,
    renderer: iced::Renderer,
    cache: user_interface::Cache,
    clipboard: iced_winit::Clipboard,
    viewport: Viewport,
    cursor_position: Option<PhysicalPosition<f64>>,
    modifiers: ModifiersState,
    mouse_interaction: mouse::Interaction,
    theme: iced::Theme,
    style: renderer::Style,
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
    ) -> Self {
        Self {
            app,
            display_handle,
            proxy,
            window: None,
            pending_mcp: None,
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

        let mut window_settings = iced::window::Settings::default();
        window_settings.size = iced::Size::new(1024.0, 768.0);
        let scale_factor = 1.0;
        let title = self.app.title();
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
        window.set_title(&title);

        let executor = <iced::executor::Default as iced::Executor>::new()
            .map_err(iced::Error::ExecutorCreationFailed)?;
        let graphics_settings: iced::advanced::graphics::Settings =
            iced::Settings::default().into();
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
        let physical_size = non_zero_physical_size(window.inner_size());
        let mut surface =
            compositor.create_surface(window.clone(), physical_size.width, physical_size.height);
        compositor.configure_surface(&mut surface, physical_size.width, physical_size.height);
        let renderer = compositor.create_renderer();
        let theme = iced::Theme::default(iced::theme::Mode::Light);
        let base = theme.base();

        self.window = Some(NativeIcedWindow {
            raw: window.clone(),
            compositor,
            surface,
            renderer,
            cache: user_interface::Cache::new(),
            clipboard: iced_winit::Clipboard::connect(window),
            viewport: viewport_from_window(&physical_size, scale_factor),
            cursor_position: None,
            modifiers: ModifiersState::default(),
            mouse_interaction: mouse::Interaction::None,
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

    fn dispatch_iced_events(
        &mut self,
        events: &[iced::Event],
    ) -> Vec<SlipwayIcedRuntimeMessage<W::AppMessage>> {
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
        let mouse_interaction = match state {
            user_interface::State::Updated {
                mouse_interaction,
                redraw_request: _,
                ..
            } => Some(mouse_interaction),
            user_interface::State::Outdated => None,
        };
        window.cache = user_interface.into_cache();
        let _ = window;
        if let Some(mouse_interaction) = mouse_interaction {
            self.update_mouse_interaction(mouse_interaction);
        }
        messages
    }

    fn process_messages(&mut self, messages: Vec<SlipwayIcedRuntimeMessage<W::AppMessage>>) {
        let mut should_redraw = false;
        for message in messages {
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
        let cursor = self.cursor();
        let Some(window) = self.window.as_mut() else {
            return;
        };
        let cache = std::mem::replace(&mut window.cache, user_interface::Cache::new());
        let mut user_interface = UserInterface::build(
            self.app.view::<iced::Theme, iced::Renderer>(),
            window.viewport.logical_size(),
            cache,
            &mut window.renderer,
        );
        user_interface.draw(&mut window.renderer, &window.theme, &window.style, cursor);
        window.cache = user_interface.into_cache();
        let base = window.theme.base();
        let _ = window.compositor.present(
            &mut window.renderer,
            &mut window.surface,
            &window.viewport,
            base.background_color,
            || {},
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

        match &event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                self.resize(*size);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(window) = self.window.as_mut() {
                    let physical_size = non_zero_physical_size(window.raw.inner_size());
                    window.viewport = Viewport::with_physical_size(
                        iced::Size::new(physical_size.width, physical_size.height),
                        *scale_factor as f32,
                    );
                    window.compositor.configure_surface(
                        &mut window.surface,
                        physical_size.width,
                        physical_size.height,
                    );
                }
                self.sync_presented_viewport_from_window();
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
                self.redraw();
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
            let messages = self.dispatch_iced_events(&[event]);
            self.process_messages(messages);
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        let Some(window) = self.window.as_mut() else {
            return;
        };
        let physical_size = non_zero_physical_size(size);
        window.viewport = Viewport::with_physical_size(
            iced::Size::new(physical_size.width, physical_size.height),
            window.raw.scale_factor() as f32,
        );
        window.compositor.configure_surface(
            &mut window.surface,
            physical_size.width,
            physical_size.height,
        );
        let raw = window.raw.clone();
        let _ = window;
        self.sync_presented_viewport_from_window();
        raw.request_redraw();
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
            DebugPhysicalControl::Text { selector, .. }
            | DebugPhysicalControl::TextEdit { selector, .. }
            | DebugPhysicalControl::Keyboard { selector, .. } => {
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

        let Some(backend_input) = matched else {
            return DebugReplyProduct::Error(DebugFailure {
                code: "native-physical-control-produced-no-backend-input".to_string(),
                message: "the synthesized iced native event reached UserInterface::update but produced no backend-presented input evidence matching the requested physical operation".to_string(),
                dispatch_evidence: None,
            });
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

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(transport) = self.app.debug_mcp_transport.as_ref() {
            if transport.drain_wakes() > 0 {
                self.pump_mcp();
            }
        }
    }
}

fn non_zero_physical_size(size: PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}

fn viewport_from_window(size: &PhysicalSize<u32>, scale_factor: f32) -> Viewport {
    let physical_size = non_zero_physical_size(*size);
    Viewport::with_physical_size(
        iced::Size::new(physical_size.width, physical_size.height),
        scale_factor,
    )
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
