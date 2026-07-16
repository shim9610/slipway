//! Connect a window with a renderer.
use crate::core::Color;
use crate::graphics::color;
use crate::graphics::compositor;
use crate::graphics::error;
use crate::graphics::{self, Shell, Viewport};
use crate::settings::{self, Settings};
use crate::{Engine, Renderer};

/// A window graphics backend for iced powered by `wgpu`.
pub struct Compositor {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    engine: Engine,
    settings: Settings,
}

/// Wakes the native event loop after a direct capture event is sent.
pub trait DirectCaptureWake: Send + Sync + 'static {
    /// Wakes the request identified by `token`.
    fn wake(&self, token: u64);
}

/// A one-shot request to copy the next acquired surface texture.
pub struct DirectCaptureRequest {
    /// Correlates capture events with their request.
    pub token: u64,
    /// Receives the bounded events produced by this capture.
    pub event_tx: std::sync::mpsc::SyncSender<DirectCaptureEvent>,
    /// Wakes the native event loop after an event is sent.
    pub wake: std::sync::Arc<dyn DirectCaptureWake>,
}

impl std::fmt::Debug for DirectCaptureRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectCaptureRequest")
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

/// An event produced by a one-shot direct surface capture.
#[derive(Debug)]
pub enum DirectCaptureEvent {
    /// The copied surface texture was passed to `SurfaceTexture::present`.
    Presented {
        /// The request token.
        token: u64,
        /// The copied surface format.
        format: DirectCaptureFormat,
        /// The copied surface alpha mode.
        alpha: DirectCaptureAlphaMode,
        /// The copied width in physical pixels.
        width: u32,
        /// The copied height in physical pixels.
        height: u32,
    },
    /// The staging buffer mapping completed.
    Mapped {
        /// The request token.
        token: u64,
        /// Tightly packed, top-row-first RGBA8 bytes or a mapping error.
        result: Result<std::sync::Arc<[u8]>, DirectCaptureMapError>,
    },
    /// The request-only device poll failed.
    PollFailed {
        /// The request token.
        token: u64,
        /// The polling error.
        error: DirectCapturePollError,
    },
    /// The concrete surface cannot perform the requested capture.
    Refused {
        /// The request token.
        token: u64,
        /// The concrete refusal reason.
        reason: DirectCaptureRefusal,
    },
}

/// A supported directly copied surface format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectCaptureFormat {
    /// Linear RGBA8.
    Rgba8Unorm,
    /// sRGB RGBA8.
    Rgba8UnormSrgb,
    /// Linear BGRA8.
    Bgra8Unorm,
    /// sRGB BGRA8.
    Bgra8UnormSrgb,
}

/// The alpha interpretation of directly copied surface pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectCaptureAlphaMode {
    /// The presented surface is opaque.
    Opaque,
    /// RGB channels are premultiplied by alpha.
    Premultiplied,
}

/// A staging-buffer mapping failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectCaptureMapError {
    /// The GPU buffer could not be mapped or read.
    MapFailed,
    /// Checked byte-length arithmetic overflowed.
    ByteLengthOverflow,
}

/// A request-only device polling failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectCapturePollError {
    /// The bounded wait expired.
    Timeout,
    /// The device rejected the wait operation.
    Device,
}

/// A concrete reason why direct capture cannot be attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectCaptureRefusal {
    /// The surface cannot be configured with `COPY_SRC`.
    CopySrcUnsupported,
    /// The surface format cannot be normalized to RGBA8.
    FormatUnsupported,
    /// The surface alpha mode has no supported evidence representation.
    AlphaUnsupported,
    /// The requested viewport has no pixels.
    ZeroSize,
    /// The requested viewport is unavailable.
    ViewportUnavailable,
    /// A capture request is already armed.
    AlreadyArmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirectCaptureLayout {
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    buffer_size: u64,
    tight_len: usize,
}

/// A compositor error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// The surface creation failed.
    #[error("the surface creation failed: {0}")]
    SurfaceCreationFailed(#[from] wgpu::CreateSurfaceError),
    /// The surface is not compatible.
    #[error("the surface is not compatible")]
    IncompatibleSurface,
    /// No adapter was found for the options requested.
    #[error("no adapter was found for the options requested: {0:?}")]
    NoAdapterFound(String),
    /// No device request succeeded.
    #[error("no device request succeeded: {0:?}")]
    RequestDeviceFailed(Vec<(wgpu::Limits, wgpu::RequestDeviceError)>),
}

impl From<Error> for graphics::Error {
    fn from(error: Error) -> Self {
        Self::GraphicsAdapterNotFound {
            backend: "wgpu",
            reason: error::Reason::RequestFailed(error.to_string()),
        }
    }
}

impl Compositor {
    /// Requests a new [`Compositor`] with the given [`Settings`].
    ///
    /// Returns `None` if no compatible graphics adapter could be found.
    pub async fn request<W: compositor::Window>(
        settings: Settings,
        compatible_window: Option<W>,
        shell: Shell,
    ) -> Result<Self, Error> {
        let instance = wgpu::util::new_instance_with_webgpu_detection(
            &wgpu::InstanceDescriptor {
                backends: settings.backends,
                flags: if cfg!(feature = "strict-assertions") {
                    wgpu::InstanceFlags::debugging()
                } else {
                    wgpu::InstanceFlags::empty()
                },
                ..Default::default()
            },
        )
        .await;

        log::info!("{settings:#?}");

        #[cfg(not(target_arch = "wasm32"))]
        if log::max_level() >= log::LevelFilter::Info {
            let available_adapters: Vec<_> = instance
                .enumerate_adapters(settings.backends)
                .iter()
                .map(wgpu::Adapter::get_info)
                .collect();
            log::info!("Available adapters: {available_adapters:#?}");
        }

        #[allow(unsafe_code)]
        let compatible_surface = compatible_window
            .and_then(|window| instance.create_surface(window).ok());

        let adapter_options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::from_env()
                .unwrap_or(wgpu::PowerPreference::HighPerformance),
            compatible_surface: compatible_surface.as_ref(),
            force_fallback_adapter: false,
        };

        let adapter =
            instance.request_adapter(&adapter_options).await.map_err(
                |_error| Error::NoAdapterFound(format!("{adapter_options:?}")),
            )?;

        log::info!("Selected: {:#?}", adapter.get_info());

        let (format, alpha_mode) = compatible_surface
            .as_ref()
            .and_then(|surface| {
                let capabilities = surface.get_capabilities(&adapter);

                let formats = capabilities.formats.iter().copied();

                log::info!("Available formats: {formats:#?}");

                let mut formats = formats.filter(|format| {
                    format.required_features() == wgpu::Features::empty()
                });

                let format = if color::GAMMA_CORRECTION {
                    formats.find(wgpu::TextureFormat::is_srgb)
                } else {
                    formats.find(|format| !wgpu::TextureFormat::is_srgb(format))
                };

                let format = format.or_else(|| {
                    log::warn!("No format found!");

                    capabilities.formats.first().copied()
                });

                let alpha_modes = capabilities.alpha_modes;

                log::info!("Available alpha modes: {alpha_modes:#?}");

                let preferred_alpha = if alpha_modes
                    .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
                {
                    wgpu::CompositeAlphaMode::PostMultiplied
                } else if alpha_modes
                    .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
                {
                    wgpu::CompositeAlphaMode::PreMultiplied
                } else {
                    wgpu::CompositeAlphaMode::Auto
                };

                format.zip(Some(preferred_alpha))
            })
            .ok_or(Error::IncompatibleSurface)?;

        log::info!(
            "Selected format: {format:?} with alpha mode: {alpha_mode:?}"
        );

        #[cfg(target_arch = "wasm32")]
        let limits = [wgpu::Limits::downlevel_webgl2_defaults()
            .using_resolution(adapter.limits())];

        #[cfg(not(target_arch = "wasm32"))]
        let limits =
            [wgpu::Limits::default(), wgpu::Limits::downlevel_defaults()];

        let limits = limits.into_iter().map(|limits| wgpu::Limits {
            max_bind_groups: 2,
            max_non_sampler_bindings: 2048,
            ..limits
        });

        let mut errors = Vec::new();

        for required_limits in limits {
            let result = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some(
                        "iced_wgpu::window::compositor device descriptor",
                    ),
                    required_features: wgpu::Features::empty(),
                    required_limits: required_limits.clone(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(
                    ),
                })
                .await;

            match result {
                Ok((device, queue)) => {
                    let engine = Engine::new(
                        &adapter,
                        device,
                        queue,
                        format,
                        settings.antialiasing,
                        shell,
                    );

                    return Ok(Compositor {
                        instance,
                        adapter,
                        format,
                        alpha_mode,
                        engine,
                        settings,
                    });
                }
                Err(error) => {
                    errors.push((required_limits, error));
                }
            }
        }

        Err(Error::RequestDeviceFailed(errors))
    }

    /// Presents normally while directly copying the acquired surface texture.
    ///
    /// The copy is request-scoped and is encoded after the visible render
    /// submission and before the same acquired texture is presented.
    pub fn present_with_direct_capture(
        &mut self,
        renderer: &mut Renderer,
        surface: &mut wgpu::Surface<'static>,
        viewport: &Viewport,
        background_color: Color,
        on_pre_present: impl FnOnce(),
        request: DirectCaptureRequest,
    ) -> Result<(), compositor::SurfaceError> {
        let size = viewport.physical_size();

        if size.width == 0 || size.height == 0 {
            return present_and_refuse(
                renderer,
                surface,
                viewport,
                background_color,
                on_pre_present,
                request,
                DirectCaptureRefusal::ZeroSize,
            );
        }

        let capabilities = surface.get_capabilities(&self.adapter);

        if !capabilities.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            return present_and_refuse(
                renderer,
                surface,
                viewport,
                background_color,
                on_pre_present,
                request,
                DirectCaptureRefusal::CopySrcUnsupported,
            );
        }

        let Some(format) = direct_capture_format(self.format) else {
            return present_and_refuse(
                renderer,
                surface,
                viewport,
                background_color,
                on_pre_present,
                request,
                DirectCaptureRefusal::FormatUnsupported,
            );
        };

        let Some(alpha) = direct_capture_alpha(self.alpha_mode) else {
            return present_and_refuse(
                renderer,
                surface,
                viewport,
                background_color,
                on_pre_present,
                request,
                DirectCaptureRefusal::AlphaUnsupported,
            );
        };

        let capture_configuration = self.surface_configuration(
            size.width,
            size.height,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
        );
        surface.configure(&self.engine.device, &capture_configuration);

        let frame = match surface.get_current_texture() {
            Ok(frame) => frame,
            Err(error) => {
                self.restore_surface_configuration(
                    surface,
                    size.width,
                    size.height,
                );

                return Err(map_surface_error(error));
            }
        };

        let width = frame.texture.width();
        let height = frame.texture.height();
        let layout = match direct_capture_layout(width, height) {
            Ok(layout) => layout,
            Err(error) => {
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let _submission = renderer.present(
                    Some(background_color),
                    frame.texture.format(),
                    &view,
                    viewport,
                );

                on_pre_present();
                frame.present();
                self.restore_surface_configuration(surface, width, height);

                send_direct_capture_event(
                    &request.event_tx,
                    &request.wake,
                    request.token,
                    DirectCaptureEvent::Presented {
                        token: request.token,
                        format,
                        alpha,
                        width,
                        height,
                    },
                );
                send_direct_capture_event(
                    &request.event_tx,
                    &request.wake,
                    request.token,
                    DirectCaptureEvent::Mapped {
                        token: request.token,
                        result: Err(error),
                    },
                );

                return Ok(());
            }
        };

        let staging =
            self.engine.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("iced_wgpu direct surface capture staging buffer"),
                size: layout.buffer_size,
                usage: wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let _render_submission = renderer.present(
            Some(background_color),
            frame.texture.format(),
            &view,
            viewport,
        );

        let mut encoder = self.engine.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("iced_wgpu direct surface capture encoder"),
            },
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &frame.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layout.padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let submission_index = self.engine.queue.submit([encoder.finish()]);

        let callback_buffer = staging.clone();
        let callback_tx = request.event_tx.clone();
        let callback_wake = std::sync::Arc::clone(&request.wake);
        let token = request.token;
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let result = result
                    .map_err(|_| DirectCaptureMapError::MapFailed)
                    .and_then(|()| {
                        let mapped =
                            callback_buffer.slice(..).get_mapped_range();
                        let pixels = normalize_direct_capture_bytes(
                            &mapped, layout, format,
                        );
                        drop(mapped);
                        callback_buffer.unmap();
                        pixels
                    });

                send_direct_capture_event(
                    &callback_tx,
                    &callback_wake,
                    token,
                    DirectCaptureEvent::Mapped { token, result },
                );
            });

        #[cfg(not(target_arch = "wasm32"))]
        {
            let device = self.engine.device.clone();
            let poll_tx = request.event_tx.clone();
            let poll_wake = std::sync::Arc::clone(&request.wake);
            let spawn_result = std::thread::Builder::new()
                .name(format!("iced-direct-capture-{token}"))
                .spawn(move || {
                    let result = device.poll(wgpu::PollType::Wait {
                        submission_index: Some(submission_index),
                        timeout: Some(std::time::Duration::from_secs(5)),
                    });

                    if let Err(error) = result {
                        let error = match error {
                            wgpu::PollError::Timeout => {
                                DirectCapturePollError::Timeout
                            }
                            wgpu::PollError::WrongSubmissionIndex(_, _) => {
                                DirectCapturePollError::Device
                            }
                        };
                        send_direct_capture_event(
                            &poll_tx,
                            &poll_wake,
                            token,
                            DirectCaptureEvent::PollFailed { token, error },
                        );
                    }
                });

            if spawn_result.is_err() {
                send_direct_capture_event(
                    &request.event_tx,
                    &request.wake,
                    token,
                    DirectCaptureEvent::PollFailed {
                        token,
                        error: DirectCapturePollError::Device,
                    },
                );
            }
        }

        #[cfg(target_arch = "wasm32")]
        let _submission_index = submission_index;

        on_pre_present();
        frame.present();
        self.restore_surface_configuration(surface, width, height);

        send_direct_capture_event(
            &request.event_tx,
            &request.wake,
            token,
            DirectCaptureEvent::Presented {
                token,
                format,
                alpha,
                width,
                height,
            },
        );

        Ok(())
    }

    fn surface_configuration(
        &self,
        width: u32,
        height: u32,
        usage: wgpu::TextureUsages,
    ) -> wgpu::SurfaceConfiguration {
        wgpu::SurfaceConfiguration {
            usage,
            format: self.format,
            present_mode: self.settings.present_mode,
            width,
            height,
            alpha_mode: self.alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        }
    }

    fn restore_surface_configuration(
        &self,
        surface: &wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) {
        surface.configure(
            &self.engine.device,
            &self.surface_configuration(
                width,
                height,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
        );
    }
}

fn direct_capture_format(
    format: wgpu::TextureFormat,
) -> Option<DirectCaptureFormat> {
    match format {
        wgpu::TextureFormat::Rgba8Unorm => {
            Some(DirectCaptureFormat::Rgba8Unorm)
        }
        wgpu::TextureFormat::Rgba8UnormSrgb => {
            Some(DirectCaptureFormat::Rgba8UnormSrgb)
        }
        wgpu::TextureFormat::Bgra8Unorm => {
            Some(DirectCaptureFormat::Bgra8Unorm)
        }
        wgpu::TextureFormat::Bgra8UnormSrgb => {
            Some(DirectCaptureFormat::Bgra8UnormSrgb)
        }
        _ => None,
    }
}

fn direct_capture_alpha(
    alpha_mode: wgpu::CompositeAlphaMode,
) -> Option<DirectCaptureAlphaMode> {
    match alpha_mode {
        wgpu::CompositeAlphaMode::Auto | wgpu::CompositeAlphaMode::Opaque => {
            Some(DirectCaptureAlphaMode::Opaque)
        }
        wgpu::CompositeAlphaMode::PreMultiplied => {
            Some(DirectCaptureAlphaMode::Premultiplied)
        }
        wgpu::CompositeAlphaMode::PostMultiplied
        | wgpu::CompositeAlphaMode::Inherit => None,
    }
}

fn direct_capture_layout(
    width: u32,
    height: u32,
) -> Result<DirectCaptureLayout, DirectCaptureMapError> {
    let unpadded_bytes_per_row = width
        .checked_mul(4)
        .ok_or(DirectCaptureMapError::ByteLengthOverflow)?;
    let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row
        .checked_add(alignment - 1)
        .ok_or(DirectCaptureMapError::ByteLengthOverflow)?
        / alignment
        * alignment;
    let buffer_size = u64::from(padded_bytes_per_row)
        .checked_mul(u64::from(height))
        .ok_or(DirectCaptureMapError::ByteLengthOverflow)?;
    let tight_len = usize::try_from(
        u64::from(unpadded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or(DirectCaptureMapError::ByteLengthOverflow)?,
    )
    .map_err(|_| DirectCaptureMapError::ByteLengthOverflow)?;

    Ok(DirectCaptureLayout {
        unpadded_bytes_per_row,
        padded_bytes_per_row,
        buffer_size,
        tight_len,
    })
}

fn normalize_direct_capture_bytes(
    mapped: &[u8],
    layout: DirectCaptureLayout,
    format: DirectCaptureFormat,
) -> Result<std::sync::Arc<[u8]>, DirectCaptureMapError> {
    let padded_row = usize::try_from(layout.padded_bytes_per_row)
        .map_err(|_| DirectCaptureMapError::ByteLengthOverflow)?;
    let unpadded_row = usize::try_from(layout.unpadded_bytes_per_row)
        .map_err(|_| DirectCaptureMapError::ByteLengthOverflow)?;
    let required = usize::try_from(layout.buffer_size)
        .map_err(|_| DirectCaptureMapError::ByteLengthOverflow)?;

    if mapped.len() < required {
        return Err(DirectCaptureMapError::MapFailed);
    }

    let mut pixels = Vec::with_capacity(layout.tight_len);

    for row in mapped[..required].chunks_exact(padded_row) {
        let row = &row[..unpadded_row];

        match format {
            DirectCaptureFormat::Rgba8Unorm
            | DirectCaptureFormat::Rgba8UnormSrgb => {
                pixels.extend_from_slice(row);
            }
            DirectCaptureFormat::Bgra8Unorm
            | DirectCaptureFormat::Bgra8UnormSrgb => {
                for pixel in row.chunks_exact(4) {
                    pixels.extend_from_slice(&[
                        pixel[2], pixel[1], pixel[0], pixel[3],
                    ]);
                }
            }
        }
    }

    Ok(std::sync::Arc::from(pixels))
}

fn send_direct_capture_event(
    event_tx: &std::sync::mpsc::SyncSender<DirectCaptureEvent>,
    wake: &std::sync::Arc<dyn DirectCaptureWake>,
    token: u64,
    event: DirectCaptureEvent,
) {
    if event_tx.send(event).is_ok() {
        wake.wake(token);
    }
}

fn present_and_refuse(
    renderer: &mut Renderer,
    surface: &mut wgpu::Surface<'static>,
    viewport: &Viewport,
    background_color: Color,
    on_pre_present: impl FnOnce(),
    request: DirectCaptureRequest,
    reason: DirectCaptureRefusal,
) -> Result<(), compositor::SurfaceError> {
    let result = present(
        renderer,
        surface,
        viewport,
        background_color,
        on_pre_present,
    );

    if result.is_ok() {
        send_direct_capture_event(
            &request.event_tx,
            &request.wake,
            request.token,
            DirectCaptureEvent::Refused {
                token: request.token,
                reason,
            },
        );
    }

    result
}

fn map_surface_error(error: wgpu::SurfaceError) -> compositor::SurfaceError {
    match error {
        wgpu::SurfaceError::Timeout => compositor::SurfaceError::Timeout,
        wgpu::SurfaceError::Outdated => compositor::SurfaceError::Outdated,
        wgpu::SurfaceError::Lost => compositor::SurfaceError::Lost,
        wgpu::SurfaceError::OutOfMemory => {
            compositor::SurfaceError::OutOfMemory
        }
        wgpu::SurfaceError::Other => compositor::SurfaceError::Other,
    }
}

/// Creates a [`Compositor`] with the given [`Settings`] and window.
pub async fn new<W: compositor::Window>(
    settings: Settings,
    compatible_window: W,
    shell: Shell,
) -> Result<Compositor, Error> {
    Compositor::request(settings, Some(compatible_window), shell).await
}

/// Presents the given primitives with the given [`Compositor`].
pub fn present(
    renderer: &mut Renderer,
    surface: &mut wgpu::Surface<'static>,
    viewport: &Viewport,
    background_color: Color,
    on_pre_present: impl FnOnce(),
) -> Result<(), compositor::SurfaceError> {
    match surface.get_current_texture() {
        Ok(frame) => {
            let view = &frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            let _submission = renderer.present(
                Some(background_color),
                frame.texture.format(),
                view,
                viewport,
            );

            // Present the frame
            on_pre_present();
            frame.present();

            Ok(())
        }
        Err(error) => match error {
            wgpu::SurfaceError::Timeout => {
                Err(compositor::SurfaceError::Timeout)
            }
            wgpu::SurfaceError::Outdated => {
                Err(compositor::SurfaceError::Outdated)
            }
            wgpu::SurfaceError::Lost => Err(compositor::SurfaceError::Lost),
            wgpu::SurfaceError::OutOfMemory => {
                Err(compositor::SurfaceError::OutOfMemory)
            }
            wgpu::SurfaceError::Other => Err(compositor::SurfaceError::Other),
        },
    }
}

impl graphics::Compositor for Compositor {
    type Renderer = Renderer;
    type Surface = wgpu::Surface<'static>;

    async fn with_backend(
        settings: graphics::Settings,
        _display: impl compositor::Display,
        compatible_window: impl compositor::Window,
        shell: Shell,
        backend: Option<&str>,
    ) -> Result<Self, graphics::Error> {
        match backend {
            None | Some("wgpu") => {
                let mut settings = Settings::from(settings);

                if let Some(backends) = wgpu::Backends::from_env() {
                    settings.backends = backends;
                }

                if let Some(present_mode) = settings::present_mode_from_env() {
                    settings.present_mode = present_mode;
                }

                Ok(new(settings, compatible_window, shell).await?)
            }
            Some(backend) => Err(graphics::Error::GraphicsAdapterNotFound {
                backend: "wgpu",
                reason: error::Reason::DidNotMatch {
                    preferred_backend: backend.to_owned(),
                },
            }),
        }
    }

    fn create_renderer(&self) -> Self::Renderer {
        Renderer::new(
            self.engine.clone(),
            self.settings.default_font,
            self.settings.default_text_size,
        )
    }

    fn create_surface<W: compositor::Window>(
        &mut self,
        window: W,
        width: u32,
        height: u32,
    ) -> Self::Surface {
        let mut surface = self
            .instance
            .create_surface(window)
            .expect("Create surface");

        if width > 0 && height > 0 {
            self.configure_surface(&mut surface, width, height);
        }

        surface
    }

    fn configure_surface(
        &mut self,
        surface: &mut Self::Surface,
        width: u32,
        height: u32,
    ) {
        surface.configure(
            &self.engine.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.format,
                present_mode: self.settings.present_mode,
                width,
                height,
                alpha_mode: self.alpha_mode,
                view_formats: vec![],
                desired_maximum_frame_latency: 1,
            },
        );
    }

    fn information(&self) -> compositor::Information {
        let information = self.adapter.get_info();

        compositor::Information {
            adapter: information.name,
            backend: format!("{:?}", information.backend),
        }
    }

    fn present(
        &mut self,
        renderer: &mut Self::Renderer,
        surface: &mut Self::Surface,
        viewport: &Viewport,
        background_color: Color,
        on_pre_present: impl FnOnce(),
    ) -> Result<(), compositor::SurfaceError> {
        present(
            renderer,
            surface,
            viewport,
            background_color,
            on_pre_present,
        )
    }

    fn screenshot(
        &mut self,
        renderer: &mut Self::Renderer,
        viewport: &Viewport,
        background_color: Color,
    ) -> Vec<u8> {
        renderer.screenshot(viewport, background_color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestWake(AtomicU64);

    impl DirectCaptureWake for TestWake {
        fn wake(&self, token: u64) {
            self.0.store(token, Ordering::SeqCst);
        }
    }

    #[test]
    fn row_layout_is_checked_and_256_byte_aligned() {
        let layout = direct_capture_layout(3, 2).expect("valid layout");

        assert_eq!(layout.unpadded_bytes_per_row, 12);
        assert_eq!(layout.padded_bytes_per_row, 256);
        assert_eq!(layout.buffer_size, 512);
        assert_eq!(layout.tight_len, 24);
        assert_eq!(
            direct_capture_layout(u32::MAX, 1),
            Err(DirectCaptureMapError::ByteLengthOverflow)
        );
    }

    #[test]
    fn rgba_normalization_strips_row_padding() {
        let layout = direct_capture_layout(1, 2).expect("valid layout");
        let mut mapped = vec![0; layout.buffer_size as usize];
        mapped[..4].copy_from_slice(&[1, 2, 3, 4]);
        mapped[256..260].copy_from_slice(&[5, 6, 7, 8]);

        let pixels = normalize_direct_capture_bytes(
            &mapped,
            layout,
            DirectCaptureFormat::Rgba8Unorm,
        )
        .expect("valid mapped bytes");

        assert_eq!(&*pixels, &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn bgra_normalization_swizzles_to_rgba() {
        let layout = direct_capture_layout(2, 1).expect("valid layout");
        let mut mapped = vec![0; layout.buffer_size as usize];
        mapped[..8].copy_from_slice(&[3, 2, 1, 4, 7, 6, 5, 8]);

        let pixels = normalize_direct_capture_bytes(
            &mapped,
            layout,
            DirectCaptureFormat::Bgra8UnormSrgb,
        )
        .expect("valid mapped bytes");

        assert_eq!(&*pixels, &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn mapped_length_is_validated() {
        let layout = direct_capture_layout(1, 1).expect("valid layout");

        assert_eq!(
            normalize_direct_capture_bytes(
                &[0; 4],
                layout,
                DirectCaptureFormat::Rgba8Unorm,
            ),
            Err(DirectCaptureMapError::MapFailed)
        );
    }

    #[test]
    fn supported_surface_metadata_is_exact() {
        assert_eq!(
            direct_capture_format(wgpu::TextureFormat::Rgba8UnormSrgb),
            Some(DirectCaptureFormat::Rgba8UnormSrgb)
        );
        assert_eq!(
            direct_capture_format(wgpu::TextureFormat::Bgra8Unorm),
            Some(DirectCaptureFormat::Bgra8Unorm)
        );
        assert_eq!(
            direct_capture_format(wgpu::TextureFormat::Rgba16Float),
            None
        );
        assert_eq!(
            direct_capture_alpha(wgpu::CompositeAlphaMode::Opaque),
            Some(DirectCaptureAlphaMode::Opaque)
        );
        assert_eq!(
            direct_capture_alpha(wgpu::CompositeAlphaMode::PreMultiplied),
            Some(DirectCaptureAlphaMode::Premultiplied)
        );
        assert_eq!(
            direct_capture_alpha(wgpu::CompositeAlphaMode::PostMultiplied),
            None
        );
    }

    #[test]
    fn event_is_sent_and_then_wakes_the_token() {
        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(1);
        let test_wake = std::sync::Arc::new(TestWake(AtomicU64::new(0)));
        let wake: std::sync::Arc<dyn DirectCaptureWake> = test_wake.clone();

        send_direct_capture_event(
            &event_tx,
            &wake,
            42,
            DirectCaptureEvent::Refused {
                token: 42,
                reason: DirectCaptureRefusal::CopySrcUnsupported,
            },
        );

        assert!(matches!(
            event_rx.try_recv(),
            Ok(DirectCaptureEvent::Refused { token: 42, .. })
        ));
        assert_eq!(test_wake.0.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn direct_hook_source_orders_render_copy_present_and_event() {
        let source = include_str!("compositor.rs");
        let hook = source
            .split_once("pub fn present_with_direct_capture")
            .expect("direct hook exists")
            .1;
        let render = hook.find("renderer.present").expect("visible render");
        let copy = hook
            .find("encoder.copy_texture_to_buffer")
            .expect("direct texture copy");
        let present = hook[copy..]
            .find("frame.present()")
            .expect("surface present after copy")
            + copy;
        let presented_event = hook[present..]
            .find("DirectCaptureEvent::Presented")
            .expect("presented event after present")
            + present;

        assert!(render < copy);
        assert!(copy < present);
        assert!(present < presented_event);
    }

    #[test]
    fn ordinary_present_has_no_capture_work() {
        let source = include_str!("compositor.rs");
        let ordinary = source
            .split_once("pub fn present(\n")
            .expect("ordinary present exists")
            .1
            .split_once("impl graphics::Compositor")
            .expect("ordinary present boundary")
            .0;

        assert!(!ordinary.contains("DirectCapture"));
        assert!(!ordinary.contains("COPY_SRC"));
        assert!(!ordinary.contains("map_async"));
    }

    #[allow(dead_code)]
    fn compile_fixture_calls_exported_hook(
        compositor: &mut Compositor,
        renderer: &mut Renderer,
        surface: &mut wgpu::Surface<'static>,
        viewport: &Viewport,
        request: DirectCaptureRequest,
    ) -> Result<(), compositor::SurfaceError> {
        compositor.present_with_direct_capture(
            renderer,
            surface,
            viewport,
            Color::TRANSPARENT,
            || {},
            request,
        )
    }
}
