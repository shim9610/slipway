#![expect(clippy::missing_errors_doc)]
#![expect(clippy::undocumented_unsafe_blocks)]
#![expect(clippy::unwrap_used)] // TODO(emilk): avoid unwraps
#![expect(unsafe_code)]

use crate::{RenderState, SurfaceConfig, SurfaceErrorAction, WgpuConfiguration, renderer};
use crate::{
    RendererOptions,
    capture::{CaptureReceiver, CaptureSender, CaptureState, capture_channel},
};
use egui::{Context, Event, UserData, ViewportId, ViewportIdMap, ViewportIdSet};
use std::{num::NonZeroU32, sync::Arc};

#[cfg(feature = "slipway_debug")]
pub trait DirectCaptureWake: Send + Sync + 'static {
    fn wake(&self, token: u64);
}

#[cfg(feature = "slipway_debug")]
pub struct DirectCaptureRequest {
    pub token: u64,
    pub event_tx: std::sync::mpsc::SyncSender<DirectCaptureEvent>,
    pub wake: Arc<dyn DirectCaptureWake>,
}

#[cfg(feature = "slipway_debug")]
impl std::fmt::Debug for DirectCaptureRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectCaptureRequest")
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "slipway_debug")]
#[derive(Debug)]
pub enum DirectCaptureEvent {
    Presented {
        token: u64,
        format: DirectCaptureFormat,
        alpha: DirectCaptureAlphaMode,
        width: u32,
        height: u32,
    },
    Mapped {
        token: u64,
        result: Result<Arc<[u8]>, DirectCaptureMapError>,
    },
    PollFailed {
        token: u64,
        error: DirectCapturePollError,
    },
    Refused {
        token: u64,
        reason: DirectCaptureRefusal,
    },
}

#[cfg(feature = "slipway_debug")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectCaptureFormat {
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Bgra8Unorm,
    Bgra8UnormSrgb,
}

#[cfg(feature = "slipway_debug")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectCaptureAlphaMode {
    Opaque,
    Premultiplied,
}

#[cfg(feature = "slipway_debug")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectCaptureMapError {
    MapFailed,
    ByteLengthOverflow,
}

#[cfg(feature = "slipway_debug")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectCapturePollError {
    Timeout,
    Device,
}

#[cfg(feature = "slipway_debug")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectCaptureRefusal {
    CopySrcUnsupported,
    FormatUnsupported,
    AlphaUnsupported,
    ZeroSize,
    ViewportUnavailable,
    SurfaceAcquireFailed,
    AlreadyArmed,
}

#[cfg(feature = "slipway_debug")]
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlipwayDebugPaintResult {
    vsync_seconds: f32,
    presented: bool,
}

#[cfg(feature = "slipway_debug")]
impl SlipwayDebugPaintResult {
    pub fn vsync_seconds(self) -> f32 {
        self.vsync_seconds
    }

    pub fn presented(self) -> bool {
        self.presented
    }
}

trait PaintResultReporter {
    type Output;

    fn finish(vsync_seconds: f32, presented: bool) -> Self::Output;
}

struct VsyncOnly;

impl PaintResultReporter for VsyncOnly {
    type Output = f32;

    #[inline]
    fn finish(vsync_seconds: f32, _presented: bool) -> Self::Output {
        vsync_seconds
    }
}

#[cfg(feature = "slipway_debug")]
struct SlipwayDebugPresentReporter;

#[cfg(feature = "slipway_debug")]
impl PaintResultReporter for SlipwayDebugPresentReporter {
    type Output = SlipwayDebugPaintResult;

    #[inline]
    fn finish(vsync_seconds: f32, presented: bool) -> Self::Output {
        SlipwayDebugPaintResult {
            vsync_seconds,
            presented,
        }
    }
}

#[cfg(feature = "slipway_debug")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectCaptureLayout {
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    buffer_size: u64,
}

#[cfg(feature = "slipway_debug")]
impl DirectCaptureLayout {
    fn new(width: u32, height: u32) -> Result<Self, DirectCaptureMapError> {
        let unpadded_bytes_per_row = width
            .checked_mul(4)
            .ok_or(DirectCaptureMapError::ByteLengthOverflow)?;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row
            .checked_add(alignment - 1)
            .map(|value| value / alignment * alignment)
            .ok_or(DirectCaptureMapError::ByteLengthOverflow)?;
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or(DirectCaptureMapError::ByteLengthOverflow)?;

        Ok(Self {
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            buffer_size,
        })
    }
}

#[cfg(feature = "slipway_debug")]
fn direct_capture_format(format: wgpu::TextureFormat) -> Option<DirectCaptureFormat> {
    match format {
        wgpu::TextureFormat::Rgba8Unorm => Some(DirectCaptureFormat::Rgba8Unorm),
        wgpu::TextureFormat::Rgba8UnormSrgb => Some(DirectCaptureFormat::Rgba8UnormSrgb),
        wgpu::TextureFormat::Bgra8Unorm => Some(DirectCaptureFormat::Bgra8Unorm),
        wgpu::TextureFormat::Bgra8UnormSrgb => Some(DirectCaptureFormat::Bgra8UnormSrgb),
        _ => None,
    }
}

#[cfg(feature = "slipway_debug")]
fn direct_capture_alpha(
    transparent_backbuffer: bool,
    alpha_mode: wgpu::CompositeAlphaMode,
) -> Option<DirectCaptureAlphaMode> {
    if !transparent_backbuffer {
        return Some(DirectCaptureAlphaMode::Opaque);
    }

    match alpha_mode {
        wgpu::CompositeAlphaMode::Opaque => Some(DirectCaptureAlphaMode::Opaque),
        wgpu::CompositeAlphaMode::PreMultiplied => Some(DirectCaptureAlphaMode::Premultiplied),
        _ => None,
    }
}

#[cfg(feature = "slipway_debug")]
fn direct_capture_tight_rgba(
    mapped: &[u8],
    layout: DirectCaptureLayout,
    height: u32,
    format: DirectCaptureFormat,
) -> Result<Arc<[u8]>, DirectCaptureMapError> {
    let tight_len = usize::try_from(
        u64::from(layout.unpadded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or(DirectCaptureMapError::ByteLengthOverflow)?,
    )
    .map_err(|_conversion_error| DirectCaptureMapError::ByteLengthOverflow)?;
    let padded_row = usize::try_from(layout.padded_bytes_per_row)
        .map_err(|_conversion_error| DirectCaptureMapError::ByteLengthOverflow)?;
    let tight_row = usize::try_from(layout.unpadded_bytes_per_row)
        .map_err(|_conversion_error| DirectCaptureMapError::ByteLengthOverflow)?;
    let mut rgba = Vec::with_capacity(tight_len);
    let swizzle = matches!(
        format,
        DirectCaptureFormat::Bgra8Unorm | DirectCaptureFormat::Bgra8UnormSrgb
    );

    for row in mapped.chunks_exact(padded_row).take(height as usize) {
        let row = &row[..tight_row];
        if swizzle {
            for pixel in row.chunks_exact(4) {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
        } else {
            rgba.extend_from_slice(row);
        }
    }

    if rgba.len() != tight_len {
        return Err(DirectCaptureMapError::MapFailed);
    }

    Ok(rgba.into())
}

#[cfg(feature = "slipway_debug")]
fn send_direct_capture_event(
    event_tx: &std::sync::mpsc::SyncSender<DirectCaptureEvent>,
    wake: &Arc<dyn DirectCaptureWake>,
    token: u64,
    event: DirectCaptureEvent,
) {
    if event_tx.send(event).is_ok() {
        wake.wake(token);
    }
}

#[cfg(feature = "slipway_debug")]
#[expect(clippy::too_many_arguments)]
fn spawn_direct_capture_poll(
    buffer: Arc<wgpu::Buffer>,
    device: wgpu::Device,
    submission_index: wgpu::SubmissionIndex,
    layout: DirectCaptureLayout,
    format: DirectCaptureFormat,
    height: u32,
    token: u64,
    event_tx: std::sync::mpsc::SyncSender<DirectCaptureEvent>,
    wake: Arc<dyn DirectCaptureWake>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    let (map_status_tx, map_status_rx) = std::sync::mpsc::sync_channel(1);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _send_result = map_status_tx.send(result);
        });

    let poll_started = std::time::Instant::now();
    std::thread::Builder::new()
        .name("egui-wgpu-direct-capture-poll".to_owned())
        .spawn(move || {
            let timeout = std::time::Duration::from_secs(5).saturating_sub(poll_started.elapsed());
            let event = match device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: Some(timeout),
            }) {
                Ok(_) => match map_status_rx.try_recv() {
                    Ok(Ok(())) => {
                        let mapped = buffer.slice(..).get_mapped_range();
                        let result = direct_capture_tight_rgba(&mapped, layout, height, format);
                        drop(mapped);
                        buffer.unmap();
                        DirectCaptureEvent::Mapped { token, result }
                    }
                    Ok(Err(_map_error)) => DirectCaptureEvent::Mapped {
                        token,
                        result: Err(DirectCaptureMapError::MapFailed),
                    },
                    Err(_missing_callback_result) => DirectCaptureEvent::PollFailed {
                        token,
                        error: DirectCapturePollError::Device,
                    },
                },
                Err(wgpu::PollError::Timeout) => DirectCaptureEvent::PollFailed {
                    token,
                    error: DirectCapturePollError::Timeout,
                },
                Err(wgpu::PollError::WrongSubmissionIndex(_, _)) => {
                    DirectCaptureEvent::PollFailed {
                        token,
                        error: DirectCapturePollError::Device,
                    }
                }
            };
            send_direct_capture_event(&event_tx, &wake, token, event);
        })
}

struct SurfaceState {
    surface: wgpu::Surface<'static>,
    alpha_mode: wgpu::CompositeAlphaMode,
    width: u32,
    height: u32,
    resizing: bool,
    needs_reconfigure: bool,
    needs_recreate: bool,
}

/// Everything you need to paint egui with [`wgpu`] on [`winit`].
///
/// Alternatively you can use [`crate::Renderer`] directly.
///
/// NOTE: all egui viewports share the same painter.
pub struct Painter {
    context: Context,
    config: WgpuConfiguration,
    options: RendererOptions,
    support_transparent_backbuffer: bool,
    screen_capture_state: Option<CaptureState>,

    instance: wgpu::Instance,
    render_state: Option<RenderState>,

    // Per viewport/window:
    depth_texture_view: ViewportIdMap<wgpu::TextureView>,
    msaa_texture_view: ViewportIdMap<wgpu::TextureView>,
    surfaces: ViewportIdMap<SurfaceState>,
    capture_tx: CaptureSender,
    capture_rx: CaptureReceiver,
}

impl Painter {
    /// Manages [`wgpu`] state, including surface state, required to render egui.
    ///
    /// Only the [`wgpu::Instance`] is initialized here. Device selection and the initialization
    /// of render + surface state is deferred until the painter is given its first window target
    /// via [`set_window()`](Self::set_window). (Ensuring that a device that's compatible with the
    /// native window is chosen)
    ///
    /// Before calling [`paint_and_update_textures()`](Self::paint_and_update_textures) a
    /// [`wgpu::Surface`] must be initialized (and corresponding render state) by calling
    /// [`set_window()`](Self::set_window) once you have
    /// a [`winit::window::Window`] with a valid `.raw_window_handle()`
    /// associated.
    pub async fn new(
        context: Context,
        config: WgpuConfiguration,
        support_transparent_backbuffer: bool,
        options: RendererOptions,
    ) -> Self {
        let (capture_tx, capture_rx) = capture_channel();
        let instance = config.wgpu_setup.new_instance().await;

        Self {
            context,
            config,
            options,
            support_transparent_backbuffer,
            screen_capture_state: None,

            instance,
            render_state: None,

            depth_texture_view: Default::default(),
            surfaces: Default::default(),
            msaa_texture_view: Default::default(),

            capture_tx,
            capture_rx,
        }
    }

    /// Get the [`RenderState`].
    ///
    /// Will return [`None`] if the render state has not been initialized yet.
    pub fn render_state(&self) -> Option<RenderState> {
        self.render_state.clone()
    }

    fn configure_surface(
        surface_state: &SurfaceState,
        render_state: &RenderState,
        config: &SurfaceConfig,
    ) {
        profiling::function_scope!();

        let SurfaceConfig {
            present_mode,
            desired_maximum_frame_latency,
        } = *config;

        // Transaction presentation can hold a drawable during AppKit live resize. Keep the
        // configured low-latency path normally, but use three Metal drawables while resizing.
        #[cfg(all(target_os = "macos", feature = "macos-window-resize-jitter-fix"))]
        let desired_maximum_frame_latency = if surface_state.resizing {
            Some(desired_maximum_frame_latency.unwrap_or(2).max(2))
        } else {
            desired_maximum_frame_latency
        };

        let width = surface_state.width;
        let height = surface_state.height;

        let mut surf_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: render_state.target_format,
            present_mode,
            alpha_mode: surface_state.alpha_mode,
            view_formats: vec![render_state.target_format],
            ..surface_state
                .surface
                .get_default_config(&render_state.adapter, width, height)
                .expect("The surface isn't supported by this adapter")
        };

        if let Some(desired_maximum_frame_latency) = desired_maximum_frame_latency {
            surf_config.desired_maximum_frame_latency = desired_maximum_frame_latency;
        }

        surface_state
            .surface
            .configure(&render_state.device, &surf_config);
    }

    #[cfg(feature = "slipway_debug")]
    fn configure_surface_for_direct_capture(
        surface_state: &SurfaceState,
        render_state: &RenderState,
        config: &SurfaceConfig,
    ) {
        let SurfaceConfig {
            present_mode,
            desired_maximum_frame_latency,
        } = *config;

        #[cfg(all(target_os = "macos", feature = "macos-window-resize-jitter-fix"))]
        let desired_maximum_frame_latency = if surface_state.resizing {
            Some(desired_maximum_frame_latency.unwrap_or(2).max(2))
        } else {
            desired_maximum_frame_latency
        };

        let mut surf_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: render_state.target_format,
            present_mode,
            alpha_mode: surface_state.alpha_mode,
            view_formats: vec![render_state.target_format],
            ..surface_state
                .surface
                .get_default_config(
                    &render_state.adapter,
                    surface_state.width,
                    surface_state.height,
                )
                .expect("The surface isn't supported by this adapter")
        };

        if let Some(desired_maximum_frame_latency) = desired_maximum_frame_latency {
            surf_config.desired_maximum_frame_latency = desired_maximum_frame_latency;
        }

        surface_state
            .surface
            .configure(&render_state.device, &surf_config);
    }

    /// Drop the existing [`wgpu::Surface`] for `viewport_id` and create a fresh one for the
    /// given window via [`wgpu::Instance::create_surface`], then configure it.
    ///
    /// Used to recover from [`wgpu::CurrentSurfaceTexture::Lost`], where reconfiguring the
    /// existing surface object cannot recover.
    fn recreate_surface(
        &mut self,
        viewport_id: ViewportId,
        window: &Arc<winit::window::Window>,
    ) -> Result<(), crate::WgpuError> {
        profiling::function_scope!();

        let Some(old_state) = self.surfaces.remove(&viewport_id) else {
            return Ok(());
        };

        let surface = self.instance.create_surface(Arc::clone(window))?;
        self.install_surface(
            surface,
            viewport_id,
            old_state.width,
            old_state.height,
            old_state.resizing,
        );
        Ok(())
    }

    /// Updates (or clears) the [`winit::window::Window`] associated with the [`Painter`]
    ///
    /// This creates a [`wgpu::Surface`] for the given Window (as well as initializing render
    /// state if needed) that is used for egui rendering.
    ///
    /// This must be called before trying to render via
    /// [`paint_and_update_textures`](Self::paint_and_update_textures)
    ///
    /// # Portability
    ///
    /// _In particular it's important to note that on Android a it's only possible to create
    /// a window surface between `Resumed` and `Paused` lifecycle events, and Winit will panic on
    /// attempts to query the raw window handle while paused._
    ///
    /// On Android [`set_window`](Self::set_window) should be called with `Some(window)` for each
    /// `Resumed` event and `None` for each `Paused` event. Currently, on all other platforms
    /// [`set_window`](Self::set_window) may be called with `Some(window)` as soon as you have a
    /// valid [`winit::window::Window`].
    ///
    /// # Errors
    /// If the provided wgpu configuration does not match an available device.
    pub async fn set_window(
        &mut self,
        viewport_id: ViewportId,
        window: Option<Arc<winit::window::Window>>,
    ) -> Result<(), crate::WgpuError> {
        profiling::scope!("Painter::set_window"); // profile_function gives bad names for async functions

        if let Some(window) = window {
            let size = window.inner_size();
            if !self.surfaces.contains_key(&viewport_id) {
                let surface = self.instance.create_surface(window)?;
                self.add_surface(surface, viewport_id, size).await?;
            }
        } else {
            log::warn!("No window - clearing all surfaces");
            self.surfaces.clear();
        }
        Ok(())
    }

    /// Updates (or clears) the [`winit::window::Window`] associated with the [`Painter`] without taking ownership of the window.
    ///
    /// Like [`set_window`](Self::set_window) except:
    ///
    /// # Safety
    /// The user is responsible for ensuring that the window is alive for as long as it is set.
    pub async unsafe fn set_window_unsafe(
        &mut self,
        viewport_id: ViewportId,
        window: Option<&winit::window::Window>,
    ) -> Result<(), crate::WgpuError> {
        profiling::scope!("Painter::set_window_unsafe"); // profile_function gives bad names for async functions

        if let Some(window) = window {
            let size = window.inner_size();
            if !self.surfaces.contains_key(&viewport_id) {
                let surface = unsafe {
                    self.instance
                        .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&window)?)?
                };
                self.add_surface(surface, viewport_id, size).await?;
            }
        } else {
            log::warn!("No window - clearing all surfaces");
            self.surfaces.clear();
        }
        Ok(())
    }

    async fn add_surface(
        &mut self,
        surface: wgpu::Surface<'static>,
        viewport_id: ViewportId,
        size: winit::dpi::PhysicalSize<u32>,
    ) -> Result<(), crate::WgpuError> {
        if self.render_state.is_none() {
            let render_state =
                RenderState::create(&self.config, &self.instance, Some(&surface), self.options)
                    .await?;
            self.render_state = Some(render_state);
        }
        self.install_surface(surface, viewport_id, size.width, size.height, false);
        Ok(())
    }

    /// Inserts a freshly created surface into [`Self::surfaces`] and configures it.
    ///
    /// Render state must already be initialised before calling this.
    // NOTE: The same assumption is already required by `resize_and_generate_depth_texture_view_and_msaa_view`.
    fn install_surface(
        &mut self,
        surface: wgpu::Surface<'static>,
        viewport_id: ViewportId,
        width: u32,
        height: u32,
        resizing: bool,
    ) {
        let alpha_mode = {
            // Panic: We use the same failure mode as `resize_and_generate_depth_texture_view_and_msaa_view`
            let render_state = self
                .render_state
                .as_ref()
                .expect("install_surface called before render_state initialization");
            if self.support_transparent_backbuffer {
                let supported_alpha_modes =
                    surface.get_capabilities(&render_state.adapter).alpha_modes;
                // Prefer pre multiplied over post multiplied!
                if supported_alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
                    wgpu::CompositeAlphaMode::PreMultiplied
                } else if supported_alpha_modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied)
                {
                    wgpu::CompositeAlphaMode::PostMultiplied
                } else {
                    log::warn!(
                        "Transparent window was requested, but the active wgpu surface does not support a `CompositeAlphaMode` with transparency."
                    );
                    wgpu::CompositeAlphaMode::Auto
                }
            } else {
                wgpu::CompositeAlphaMode::Auto
            }
        };
        self.surfaces.insert(
            viewport_id,
            SurfaceState {
                surface,
                width,
                height,
                alpha_mode,
                resizing,
                needs_reconfigure: false,
                needs_recreate: false,
            },
        );
        let Some(width) = NonZeroU32::new(width) else {
            log::debug!("The window width was zero; skipping generate textures");
            return;
        };
        let Some(height) = NonZeroU32::new(height) else {
            log::debug!("The window height was zero; skipping generate textures");
            return;
        };
        self.resize_and_generate_depth_texture_view_and_msaa_view(viewport_id, width, height);
    }

    /// Returns the maximum texture dimension supported if known
    ///
    /// This API will only return a known dimension after `set_window()` has been called
    /// at least once, since the underlying device and render state are initialized lazily
    /// once we have a window (that may determine the choice of adapter/device).
    pub fn max_texture_side(&self) -> Option<usize> {
        self.render_state
            .as_ref()
            .map(|rs| rs.device.limits().max_texture_dimension_2d as usize)
    }

    fn resize_and_generate_depth_texture_view_and_msaa_view(
        &mut self,
        viewport_id: ViewportId,
        width_in_pixels: NonZeroU32,
        height_in_pixels: NonZeroU32,
    ) {
        profiling::function_scope!();

        let width = width_in_pixels.get();
        let height = height_in_pixels.get();

        let render_state = self.render_state.as_ref().unwrap();
        let surface_state = self.surfaces.get_mut(&viewport_id).unwrap();

        surface_state.width = width;
        surface_state.height = height;

        Self::configure_surface(surface_state, render_state, &self.config.surface);

        if let Some(depth_format) = self.options.depth_stencil_format {
            self.depth_texture_view.insert(
                viewport_id,
                render_state
                    .device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("egui_depth_texture"),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: self.options.msaa_samples.max(1),
                        dimension: wgpu::TextureDimension::D2,
                        format: depth_format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[depth_format],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default()),
            );
        }

        if let Some(render_state) = (self.options.msaa_samples > 1)
            .then_some(self.render_state.as_ref())
            .flatten()
        {
            let texture_format = render_state.target_format;
            self.msaa_texture_view.insert(
                viewport_id,
                render_state
                    .device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("egui_msaa_texture"),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: self.options.msaa_samples.max(1),
                        dimension: wgpu::TextureDimension::D2,
                        format: texture_format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[texture_format],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default()),
            );
        }
    }

    /// Handles changes of the resizing state.
    ///
    /// Should be called prior to the first [`Painter::on_window_resized`] call and after the last in
    /// the chain. Used to apply platform-specific logic, e.g. OSX Metal window resize jitter fix.
    pub fn on_window_resize_state_change(&mut self, viewport_id: ViewportId, resizing: bool) {
        profiling::function_scope!();

        let Some(state) = self.surfaces.get_mut(&viewport_id) else {
            return;
        };
        if state.resizing == resizing {
            if resizing {
                log::debug!(
                    "Painter::on_window_resize_state_change() redundant call while resizing"
                );
            } else {
                log::debug!(
                    "Painter::on_window_resize_state_change() redundant call after resizing"
                );
            }
            return;
        }

        // Set before reconfiguring so macOS live resize uses the temporary latency bump above.
        state.resizing = resizing;

        // Resizing is a bit tricky on macOS.
        // It requires enabling ["present_with_transaction"](https://developer.apple.com/documentation/quartzcore/cametallayer/presentswithtransaction)
        // flag to avoid jittering during the resize. Even though resize jittering on macOS
        // is common across rendering backends, the solution for wgpu/metal is known.
        //
        // See https://github.com/emilk/egui/issues/903
        #[cfg(all(target_os = "macos", feature = "macos-window-resize-jitter-fix"))]
        {
            // SAFETY: `as_hal::<Metal>()` returns `None` unless this surface is backed by wgpu's
            // Metal backend.
            unsafe {
                if let (Some(render_state), Some(hal_surface)) = (
                    self.render_state.as_ref(),
                    state.surface.as_hal::<wgpu::hal::api::Metal>(),
                ) {
                    hal_surface
                        .render_layer()
                        .lock()
                        .setPresentsWithTransaction(resizing);

                    Self::configure_surface(state, render_state, &self.config.surface);
                }
            }
        }
    }

    pub fn on_window_resized(
        &mut self,
        viewport_id: ViewportId,
        width_in_pixels: NonZeroU32,
        height_in_pixels: NonZeroU32,
    ) {
        profiling::function_scope!();

        if self.surfaces.contains_key(&viewport_id) {
            self.resize_and_generate_depth_texture_view_and_msaa_view(
                viewport_id,
                width_in_pixels,
                height_in_pixels,
            );
        } else {
            log::warn!(
                "Ignoring window resize notification with no surface created via Painter::set_window()"
            );
        }
    }

    /// Returns two things:
    ///
    /// The approximate number of seconds spent on vsync-waiting (if any),
    /// and the captures captured screenshot if it was requested.
    ///
    /// If `capture_data` isn't empty, a screenshot will be captured.
    #[expect(clippy::too_many_arguments)]
    pub fn paint_and_update_textures(
        &mut self,
        viewport_id: ViewportId,
        pixels_per_point: f32,
        clear_color: [f32; 4],
        clipped_primitives: &[epaint::ClippedPrimitive],
        textures_delta: &epaint::textures::TexturesDelta,
        capture_data: Vec<UserData>,
        window: &Arc<winit::window::Window>,
    ) -> f32 {
        self.paint_and_update_textures_impl::<VsyncOnly>(
            viewport_id,
            pixels_per_point,
            clear_color,
            clipped_primitives,
            textures_delta,
            capture_data,
            window,
        )
    }

    #[cfg(feature = "slipway_debug")]
    #[expect(clippy::too_many_arguments)]
    pub fn paint_and_update_textures_with_slipway_present_result(
        &mut self,
        viewport_id: ViewportId,
        pixels_per_point: f32,
        clear_color: [f32; 4],
        clipped_primitives: &[epaint::ClippedPrimitive],
        textures_delta: &epaint::textures::TexturesDelta,
        capture_data: Vec<UserData>,
        window: &Arc<winit::window::Window>,
    ) -> SlipwayDebugPaintResult {
        self.paint_and_update_textures_impl::<SlipwayDebugPresentReporter>(
            viewport_id,
            pixels_per_point,
            clear_color,
            clipped_primitives,
            textures_delta,
            capture_data,
            window,
        )
    }

    #[expect(clippy::too_many_arguments)]
    fn paint_and_update_textures_impl<R: PaintResultReporter>(
        &mut self,
        viewport_id: ViewportId,
        pixels_per_point: f32,
        clear_color: [f32; 4],
        clipped_primitives: &[epaint::ClippedPrimitive],
        textures_delta: &epaint::textures::TexturesDelta,
        capture_data: Vec<UserData>,
        window: &Arc<winit::window::Window>,
    ) -> R::Output {
        profiling::function_scope!();

        /// Guard to ensure that commands are always submitted to the renderer queue
        /// so that calls to [`write_buffer()`](https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#method.write_buffer)
        /// are completed even if we take a codepath which doesn't submit commands and avoids
        /// internal buffers growing indefinitely.
        ///
        /// This may happen, for example, if no output frame is resolved.
        /// See <https://github.com/emilk/egui/pull/7928> for full context.
        struct RendererQueueGuard<'q> {
            queue: &'q wgpu::Queue,
            commands_submitted: bool,
        }

        impl Drop for RendererQueueGuard<'_> {
            fn drop(&mut self) {
                // Only submit an empty command buffer array if no commands were
                // explicitly submitted.
                if !self.commands_submitted {
                    self.queue.submit([]);
                }
            }
        }

        let capture = !capture_data.is_empty();
        let mut vsync_sec = 0.0;

        // If the previous frame produced `CurrentSurfaceTexture::Lost`, the action match
        // below set `needs_recreate`. Recreate the surface now, before re-borrowing
        // `self.render_state` / `self.surfaces` for the rest of the paint.
        if self
            .surfaces
            .get(&viewport_id)
            .is_some_and(|s| s.needs_recreate)
            && let Err(err) = self.recreate_surface(viewport_id, window)
        {
            log::error!("Failed to recreate surface for {viewport_id:?}: {err}");
            return R::finish(vsync_sec, false);
        }

        // Apply any runtime changes requested via `RenderState::surface_config`.
        // We diff against the already-applied values in `self.config.surface`
        // and, if anything differs, mark every surface as needing reconfiguration so
        // the existing `needs_reconfigure` pathway below picks them up.
        if let Some(render_state) = self.render_state.as_ref()
            && render_state.surface_config != self.config.surface
        {
            self.config.surface = render_state.surface_config;
            #[expect(clippy::iter_over_hash_type)]
            for surface in self.surfaces.values_mut() {
                surface.needs_reconfigure = true;
            }
        }

        let Some(render_state) = self.render_state.as_mut() else {
            return R::finish(vsync_sec, false);
        };

        let mut render_queue_guard = RendererQueueGuard {
            queue: &render_state.queue,
            commands_submitted: false,
        };

        {
            // Upload textures before the surface-dependent early-returns below:
            // uploads only need the device + queue, and the atlas dirty region is
            // already consumed, so dropping the delta would desync the font texture.
            let mut renderer = render_state.renderer.write();
            for (id, image_delta) in &textures_delta.set {
                renderer.update_texture(
                    &render_state.device,
                    &render_state.queue,
                    *id,
                    image_delta,
                );
            }
        }

        let Some(surface_state) = self.surfaces.get_mut(&viewport_id) else {
            return R::finish(vsync_sec, false);
        };

        let mut encoder =
            render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("encoder"),
                });

        // Upload all resources for the GPU.
        let screen_descriptor = renderer::ScreenDescriptor {
            size_in_pixels: [surface_state.width, surface_state.height],
            pixels_per_point,
        };

        let user_cmd_bufs = {
            let mut renderer = render_state.renderer.write();
            renderer.update_buffers(
                &render_state.device,
                &render_state.queue,
                &mut encoder,
                clipped_primitives,
                &screen_descriptor,
            )
        };

        if surface_state.needs_reconfigure {
            Self::configure_surface(surface_state, render_state, &self.config.surface);
            surface_state.needs_reconfigure = false;
        }

        let output_frame = {
            profiling::scope!("get_current_texture");
            // This is what vsync-waiting happens on my Mac.
            let start = web_time::Instant::now();
            let output_frame = surface_state.surface.get_current_texture();
            vsync_sec += start.elapsed().as_secs_f32();
            output_frame
        };

        let output_frame = match output_frame {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                surface_state.needs_reconfigure = true;
                frame
            }
            other => {
                match (*self.config.on_surface_status)(&other) {
                    SurfaceErrorAction::Reconfigure => {
                        Self::configure_surface(surface_state, render_state, &self.config.surface);
                        self.context.request_repaint_of(viewport_id);
                    }
                    SurfaceErrorAction::RecreateSurface => {
                        // Because of ownership, I could not find an easy way to do a full recovery here,
                        // as that would involve dropping the old surface and creating a new one.
                        // For now, we defer the recreation to the beginning of the next frame (which
                        // we ensure to arrive via `request_repaint_of`). A cleaner solution would be
                        // to untangle the ownership of `RenderState`.
                        surface_state.needs_recreate = true;
                        self.context.request_repaint_of(viewport_id);
                    }
                    SurfaceErrorAction::SkipFrame => {}
                }
                return R::finish(vsync_sec, false);
            }
        };

        let mut capture_buffer = None;
        {
            let renderer = render_state.renderer.read();

            let target_texture = if capture {
                let capture_state = self.screen_capture_state.get_or_insert_with(|| {
                    CaptureState::new(&render_state.device, &output_frame.texture)
                });
                capture_state.update(&render_state.device, &output_frame.texture);

                &capture_state.texture
            } else {
                &output_frame.texture
            };
            let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

            let (view, resolve_target) = (self.options.msaa_samples > 1)
                .then_some(self.msaa_texture_view.get(&viewport_id))
                .flatten()
                .map_or((&target_view, None), |texture_view| {
                    (texture_view, Some(&target_view))
                });

            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color[0] as f64,
                            g: clear_color[1] as f64,
                            b: clear_color[2] as f64,
                            a: clear_color[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: self.depth_texture_view.get(&viewport_id).map(|view| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: self
                            .options
                            .depth_stencil_format
                            .is_some_and(|depth_stencil_format| {
                                depth_stencil_format.has_depth_aspect()
                            })
                            .then_some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                // It is very unlikely that the depth buffer is needed after egui finished rendering
                                // so no need to store it. (this can improve performance on tiling GPUs like mobile chips or Apple Silicon)
                                store: wgpu::StoreOp::Discard,
                            }),
                        stencil_ops: self
                            .options
                            .depth_stencil_format
                            .is_some_and(|depth_stencil_format| {
                                depth_stencil_format.has_stencil_aspect()
                            })
                            .then_some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(0),
                                store: wgpu::StoreOp::Discard,
                            }),
                    }
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Forgetting the pass' lifetime means that we are no longer compile-time protected from
            // runtime errors caused by accessing the parent encoder before the render pass is dropped.
            // Since we don't pass it on to the renderer, we should be perfectly safe against this mistake here!
            renderer.render(
                &mut render_pass.forget_lifetime(),
                clipped_primitives,
                &screen_descriptor,
            );

            if capture && let Some(capture_state) = &mut self.screen_capture_state {
                capture_buffer = Some(capture_state.copy_textures(
                    &render_state.device,
                    &output_frame,
                    &mut encoder,
                ));
            }
        }

        let encoded = {
            profiling::scope!("CommandEncoder::finish");
            encoder.finish()
        };

        // Submit the commands: both the main buffer and user-defined ones.
        {
            profiling::scope!("Queue::submit");
            // wgpu doesn't document where vsync can happen. Maybe here?
            let start = web_time::Instant::now();
            render_state
                .queue
                .submit(std::iter::chain(user_cmd_bufs, [encoded]));
            vsync_sec += start.elapsed().as_secs_f32();
        };

        // Ensure that the queue guard does not do unnecessary work when dropped
        render_queue_guard.commands_submitted = true;

        // Free textures marked for destruction **after** queue submit since they might still be used in the current frame.
        // Calling `wgpu::Texture::destroy` on a texture that is still in use would invalidate the command buffer(s) it is used in.
        // However, once we called `wgpu::Queue::submit`, it is up for wgpu to determine how long the underlying gpu resource has to live.
        {
            let mut renderer = render_state.renderer.write();
            for id in &textures_delta.free {
                renderer.free_texture(id);
            }
        }

        if let Some(capture_buffer) = capture_buffer
            && let Some(screen_capture_state) = &mut self.screen_capture_state
        {
            screen_capture_state.read_screen_rgba(
                self.context.clone(),
                capture_buffer,
                capture_data,
                self.capture_tx.clone(),
                viewport_id,
            );
        }

        window.pre_present_notify();

        {
            profiling::scope!("present");
            // wgpu doesn't document where vsync can happen. Maybe here?
            let start = web_time::Instant::now();
            output_frame.present();
            vsync_sec += start.elapsed().as_secs_f32();
        }

        R::finish(vsync_sec, true)
    }

    #[cfg(feature = "slipway_debug")]
    #[expect(clippy::too_many_arguments)]
    pub fn paint_and_update_textures_with_direct_capture(
        &mut self,
        viewport_id: egui::ViewportId,
        pixels_per_point: f32,
        clear_color: [f32; 4],
        clipped_primitives: &[epaint::ClippedPrimitive],
        textures_delta: &epaint::textures::TexturesDelta,
        capture_data: Vec<egui::UserData>,
        window: &Arc<winit::window::Window>,
        request: DirectCaptureRequest,
    ) -> SlipwayDebugPaintResult {
        profiling::function_scope!();

        struct RendererQueueGuard<'q> {
            queue: &'q wgpu::Queue,
            commands_submitted: bool,
        }

        impl Drop for RendererQueueGuard<'_> {
            fn drop(&mut self) {
                if !self.commands_submitted {
                    self.queue.submit([]);
                }
            }
        }

        enum CapturePlan {
            Capture {
                format: DirectCaptureFormat,
                alpha: DirectCaptureAlphaMode,
                layout: DirectCaptureLayout,
            },
            MapError {
                format: DirectCaptureFormat,
                alpha: DirectCaptureAlphaMode,
                error: DirectCaptureMapError,
            },
            Refuse(DirectCaptureRefusal),
        }

        let DirectCaptureRequest {
            token,
            event_tx,
            wake,
        } = request;
        drop(capture_data);

        let mut paint_result = SlipwayDebugPaintResult {
            vsync_seconds: 0.0,
            presented: false,
        };

        if self
            .surfaces
            .get(&viewport_id)
            .is_some_and(|surface| surface.needs_recreate)
            && let Err(err) = self.recreate_surface(viewport_id, window)
        {
            log::error!("Failed to recreate surface for {viewport_id:?}: {err}");
            send_direct_capture_event(
                &event_tx,
                &wake,
                token,
                DirectCaptureEvent::Refused {
                    token,
                    reason: DirectCaptureRefusal::ViewportUnavailable,
                },
            );
            return paint_result;
        }

        if let Some(render_state) = self.render_state.as_ref()
            && render_state.surface_config != self.config.surface
        {
            self.config.surface = render_state.surface_config;
            #[expect(clippy::iter_over_hash_type)]
            for surface in self.surfaces.values_mut() {
                surface.needs_reconfigure = true;
            }
        }

        let Some(render_state) = self.render_state.as_mut() else {
            send_direct_capture_event(
                &event_tx,
                &wake,
                token,
                DirectCaptureEvent::Refused {
                    token,
                    reason: DirectCaptureRefusal::ViewportUnavailable,
                },
            );
            return paint_result;
        };

        let mut render_queue_guard = RendererQueueGuard {
            queue: &render_state.queue,
            commands_submitted: false,
        };

        {
            let mut renderer = render_state.renderer.write();
            for (id, image_delta) in &textures_delta.set {
                renderer.update_texture(
                    &render_state.device,
                    &render_state.queue,
                    *id,
                    image_delta,
                );
            }
        }

        let Some(surface_state) = self.surfaces.get_mut(&viewport_id) else {
            send_direct_capture_event(
                &event_tx,
                &wake,
                token,
                DirectCaptureEvent::Refused {
                    token,
                    reason: DirectCaptureRefusal::ViewportUnavailable,
                },
            );
            return paint_result;
        };

        if surface_state.width == 0 || surface_state.height == 0 {
            send_direct_capture_event(
                &event_tx,
                &wake,
                token,
                DirectCaptureEvent::Refused {
                    token,
                    reason: DirectCaptureRefusal::ZeroSize,
                },
            );
            return paint_result;
        }

        let format = direct_capture_format(render_state.target_format);
        let alpha = direct_capture_alpha(
            self.support_transparent_backbuffer,
            surface_state.alpha_mode,
        );
        let capabilities = surface_state
            .surface
            .get_capabilities(&render_state.adapter);
        let capture_plan = if !capabilities.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            CapturePlan::Refuse(DirectCaptureRefusal::CopySrcUnsupported)
        } else {
            match (format, alpha) {
                (None, _) => CapturePlan::Refuse(DirectCaptureRefusal::FormatUnsupported),
                (_, None) => CapturePlan::Refuse(DirectCaptureRefusal::AlphaUnsupported),
                (Some(format), Some(alpha)) => {
                    match DirectCaptureLayout::new(surface_state.width, surface_state.height) {
                        Ok(layout) => CapturePlan::Capture {
                            format,
                            alpha,
                            layout,
                        },
                        Err(error) => CapturePlan::MapError {
                            format,
                            alpha,
                            error,
                        },
                    }
                }
            }
        };

        let direct_surface_configuration = matches!(capture_plan, CapturePlan::Capture { .. });
        if direct_surface_configuration {
            Self::configure_surface_for_direct_capture(
                surface_state,
                render_state,
                &self.config.surface,
            );
            surface_state.needs_reconfigure = false;
        } else if surface_state.needs_reconfigure {
            Self::configure_surface(surface_state, render_state, &self.config.surface);
            surface_state.needs_reconfigure = false;
        }

        let mut encoder =
            render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("encoder"),
                });

        let screen_descriptor = renderer::ScreenDescriptor {
            size_in_pixels: [surface_state.width, surface_state.height],
            pixels_per_point,
        };

        let user_cmd_bufs = {
            let mut renderer = render_state.renderer.write();
            renderer.update_buffers(
                &render_state.device,
                &render_state.queue,
                &mut encoder,
                clipped_primitives,
                &screen_descriptor,
            )
        };

        let output_frame = {
            profiling::scope!("get_current_texture");
            let start = web_time::Instant::now();
            let output_frame = surface_state.surface.get_current_texture();
            paint_result.vsync_seconds += start.elapsed().as_secs_f32();
            output_frame
        };

        let output_frame = match output_frame {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                surface_state.needs_reconfigure = true;
                frame
            }
            other => {
                if direct_surface_configuration {
                    Self::configure_surface(surface_state, render_state, &self.config.surface);
                    surface_state.needs_reconfigure = false;
                }
                match (*self.config.on_surface_status)(&other) {
                    SurfaceErrorAction::Reconfigure => {
                        Self::configure_surface(surface_state, render_state, &self.config.surface);
                        self.context.request_repaint_of(viewport_id);
                    }
                    SurfaceErrorAction::RecreateSurface => {
                        surface_state.needs_recreate = true;
                        self.context.request_repaint_of(viewport_id);
                    }
                    SurfaceErrorAction::SkipFrame => {}
                }
                send_direct_capture_event(
                    &event_tx,
                    &wake,
                    token,
                    DirectCaptureEvent::Refused {
                        token,
                        reason: DirectCaptureRefusal::SurfaceAcquireFailed,
                    },
                );
                return paint_result;
            }
        };

        {
            let renderer = render_state.renderer.read();
            let target_view = output_frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let (view, resolve_target) = (self.options.msaa_samples > 1)
                .then_some(self.msaa_texture_view.get(&viewport_id))
                .flatten()
                .map_or((&target_view, None), |texture_view| {
                    (texture_view, Some(&target_view))
                });

            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color[0] as f64,
                            g: clear_color[1] as f64,
                            b: clear_color[2] as f64,
                            a: clear_color[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: self.depth_texture_view.get(&viewport_id).map(|view| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: self
                            .options
                            .depth_stencil_format
                            .is_some_and(|depth_stencil_format| {
                                depth_stencil_format.has_depth_aspect()
                            })
                            .then_some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Discard,
                            }),
                        stencil_ops: self
                            .options
                            .depth_stencil_format
                            .is_some_and(|depth_stencil_format| {
                                depth_stencil_format.has_stencil_aspect()
                            })
                            .then_some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(0),
                                store: wgpu::StoreOp::Discard,
                            }),
                    }
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            renderer.render(
                &mut render_pass.forget_lifetime(),
                clipped_primitives,
                &screen_descriptor,
            );
        }

        let capture_buffer = if let CapturePlan::Capture { layout, .. } = capture_plan {
            let buffer = render_state.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("slipway_direct_surface_capture_buffer"),
                size: layout.buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                output_frame.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(layout.padded_bytes_per_row),
                        rows_per_image: Some(surface_state.height),
                    },
                },
                wgpu::Extent3d {
                    width: surface_state.width,
                    height: surface_state.height,
                    depth_or_array_layers: 1,
                },
            );
            Some(buffer)
        } else {
            None
        };

        let encoded = {
            profiling::scope!("CommandEncoder::finish");
            encoder.finish()
        };

        let submission_index = {
            profiling::scope!("Queue::submit");
            let start = web_time::Instant::now();
            let submission_index = render_state
                .queue
                .submit(std::iter::chain(user_cmd_bufs, [encoded]));
            paint_result.vsync_seconds += start.elapsed().as_secs_f32();
            submission_index
        };
        render_queue_guard.commands_submitted = true;

        {
            let mut renderer = render_state.renderer.write();
            for id in &textures_delta.free {
                renderer.free_texture(id);
            }
        }

        if let (Some(buffer), CapturePlan::Capture { format, layout, .. }) =
            (capture_buffer, &capture_plan)
        {
            let buffer = Arc::new(buffer);
            let spawn_result = spawn_direct_capture_poll(
                buffer,
                render_state.device.clone(),
                submission_index,
                *layout,
                *format,
                surface_state.height,
                token,
                event_tx.clone(),
                Arc::clone(&wake),
            );
            if spawn_result.is_err() {
                send_direct_capture_event(
                    &event_tx,
                    &wake,
                    token,
                    DirectCaptureEvent::PollFailed {
                        token,
                        error: DirectCapturePollError::Device,
                    },
                );
            }
        }

        window.pre_present_notify();

        {
            profiling::scope!("present");
            let start = web_time::Instant::now();
            output_frame.present();
            paint_result.vsync_seconds += start.elapsed().as_secs_f32();
        }
        paint_result.presented = true;

        if direct_surface_configuration {
            let reconfigure_next_frame = surface_state.needs_reconfigure;
            Self::configure_surface(surface_state, render_state, &self.config.surface);
            surface_state.needs_reconfigure = reconfigure_next_frame;
        }

        match capture_plan {
            CapturePlan::Capture { format, alpha, .. } => send_direct_capture_event(
                &event_tx,
                &wake,
                token,
                DirectCaptureEvent::Presented {
                    token,
                    format,
                    alpha,
                    width: surface_state.width,
                    height: surface_state.height,
                },
            ),
            CapturePlan::MapError {
                format,
                alpha,
                error,
            } => {
                send_direct_capture_event(
                    &event_tx,
                    &wake,
                    token,
                    DirectCaptureEvent::Presented {
                        token,
                        format,
                        alpha,
                        width: surface_state.width,
                        height: surface_state.height,
                    },
                );
                send_direct_capture_event(
                    &event_tx,
                    &wake,
                    token,
                    DirectCaptureEvent::Mapped {
                        token,
                        result: Err(error),
                    },
                );
            }
            CapturePlan::Refuse(reason) => send_direct_capture_event(
                &event_tx,
                &wake,
                token,
                DirectCaptureEvent::Refused { token, reason },
            ),
        }

        paint_result
    }

    /// Call this at the beginning of each frame to receive the requested screenshots.
    pub fn handle_screenshots(&self, events: &mut Vec<Event>) {
        for (viewport_id, user_data, screenshot) in self.capture_rx.try_iter() {
            let screenshot = Arc::new(screenshot);
            for data in user_data {
                events.push(Event::Screenshot {
                    viewport_id,
                    user_data: data,
                    image: Arc::clone(&screenshot),
                });
            }
        }
    }

    pub fn gc_viewports(&mut self, active_viewports: &ViewportIdSet) {
        self.surfaces.retain(|id, _| active_viewports.contains(id));
        self.depth_texture_view
            .retain(|id, _| active_viewports.contains(id));
        self.msaa_texture_view
            .retain(|id, _| active_viewports.contains(id));
    }

    #[expect(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    pub fn destroy(&mut self) {
        // TODO(emilk): something here?
    }
}

#[cfg(all(test, feature = "slipway_debug"))]
mod direct_capture_tests {
    use super::*;
    use std::{
        future::Future,
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context, Poll, Wake, Waker},
        time::{Duration, Instant},
    };

    struct CountingWake(AtomicUsize);

    impl DirectCaptureWake for CountingWake {
        fn wake(&self, token: u64) {
            self.0.fetch_add(token as usize, Ordering::Relaxed);
        }
    }

    struct ThreadWake(std::thread::Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on_with_timeout<F: Future>(future: F, timeout: Duration) -> F::Output {
        let deadline = Instant::now() + timeout;
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "WGPU initialization timed out");
            std::thread::park_timeout(remaining);
        }
    }

    #[test]
    fn direct_capture_layout_is_checked_and_256_byte_aligned() {
        let layout = DirectCaptureLayout::new(65, 3).unwrap();
        assert_eq!(layout.unpadded_bytes_per_row, 260);
        assert_eq!(layout.padded_bytes_per_row, 512);
        assert_eq!(layout.buffer_size, 1536);

        assert_eq!(
            DirectCaptureLayout::new(u32::MAX, 1),
            Err(DirectCaptureMapError::ByteLengthOverflow)
        );
    }

    #[test]
    fn padded_bgra_rows_become_tight_top_row_first_rgba() {
        let layout = DirectCaptureLayout::new(2, 2).unwrap();
        let mut mapped = vec![0_u8; layout.buffer_size as usize];
        mapped[..8].copy_from_slice(&[3, 2, 1, 4, 7, 6, 5, 8]);
        let second_row = layout.padded_bytes_per_row as usize;
        mapped[second_row..second_row + 8].copy_from_slice(&[11, 10, 9, 12, 15, 14, 13, 16]);

        let rgba =
            direct_capture_tight_rgba(&mapped, layout, 2, DirectCaptureFormat::Bgra8UnormSrgb)
                .unwrap();

        assert_eq!(
            rgba.as_ref(),
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    #[test]
    fn padded_rgba_rows_only_strip_padding() {
        let layout = DirectCaptureLayout::new(1, 2).unwrap();
        let mut mapped = vec![0_u8; layout.buffer_size as usize];
        mapped[..4].copy_from_slice(&[1, 2, 3, 4]);
        let second_row = layout.padded_bytes_per_row as usize;
        mapped[second_row..second_row + 4].copy_from_slice(&[5, 6, 7, 8]);

        let rgba =
            direct_capture_tight_rgba(&mapped, layout, 2, DirectCaptureFormat::Rgba8Unorm).unwrap();

        assert_eq!(rgba.as_ref(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn short_mapping_is_an_honest_map_failure() {
        let layout = DirectCaptureLayout::new(2, 2).unwrap();
        assert_eq!(
            direct_capture_tight_rgba(&[0; 8], layout, 2, DirectCaptureFormat::Rgba8Unorm,),
            Err(DirectCaptureMapError::MapFailed)
        );
    }

    #[test]
    fn direct_capture_classifies_only_validated_formats_and_alpha() {
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
            direct_capture_alpha(false, wgpu::CompositeAlphaMode::Auto),
            Some(DirectCaptureAlphaMode::Opaque)
        );
        assert_eq!(
            direct_capture_alpha(true, wgpu::CompositeAlphaMode::PreMultiplied),
            Some(DirectCaptureAlphaMode::Premultiplied)
        );
        assert_eq!(
            direct_capture_alpha(true, wgpu::CompositeAlphaMode::PostMultiplied),
            None
        );
    }

    #[test]
    fn wake_follows_successful_event_delivery_only() {
        let wake = Arc::new(CountingWake(AtomicUsize::new(0)));
        let wake_trait: Arc<dyn DirectCaptureWake> = Arc::clone(&wake) as Arc<_>;
        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(1);
        send_direct_capture_event(
            &event_tx,
            &wake_trait,
            7,
            DirectCaptureEvent::Refused {
                token: 7,
                reason: DirectCaptureRefusal::ViewportUnavailable,
            },
        );
        assert!(matches!(
            event_rx.recv().unwrap(),
            DirectCaptureEvent::Refused { token: 7, .. }
        ));
        assert_eq!(wake.0.load(Ordering::Relaxed), 7);

        drop(event_rx);
        send_direct_capture_event(
            &event_tx,
            &wake_trait,
            11,
            DirectCaptureEvent::PollFailed {
                token: 11,
                error: DirectCapturePollError::Device,
            },
        );
        assert_eq!(wake.0.load(Ordering::Relaxed), 7);
    }

    #[expect(dead_code, clippy::too_many_arguments)]
    fn direct_capture_api_compile_fixture(
        painter: &mut Painter,
        viewport_id: egui::ViewportId,
        pixels_per_point: f32,
        clear_color: [f32; 4],
        clipped_primitives: &[epaint::ClippedPrimitive],
        textures_delta: &epaint::textures::TexturesDelta,
        capture_data: Vec<egui::UserData>,
        window: &Arc<winit::window::Window>,
        request: DirectCaptureRequest,
    ) -> SlipwayDebugPaintResult {
        painter.paint_and_update_textures_with_direct_capture(
            viewport_id,
            pixels_per_point,
            clear_color,
            clipped_primitives,
            textures_delta,
            capture_data,
            window,
            request,
        )
    }

    #[test]
    fn direct_capture_real_device_poll_success_observes_map_callback() {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let Ok(adapter) = block_on_with_timeout(
            instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
            }),
            Duration::from_secs(10),
        ) else {
            return;
        };
        let Ok((device, queue)) = block_on_with_timeout(
            adapter.request_device(&wgpu::DeviceDescriptor::default()),
            Duration::from_secs(10),
        ) else {
            return;
        };

        let layout = DirectCaptureLayout::new(1, 1).unwrap();
        let buffer = Arc::new(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("slipway_direct_capture_real_device_test"),
            size: layout.buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        }));
        let mut padded = vec![0_u8; layout.buffer_size as usize];
        padded[..4].copy_from_slice(&[1, 2, 3, 4]);
        queue.write_buffer(&buffer, 0, &padded);
        let submission_index = queue.submit([]);

        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(1);
        let wake: Arc<dyn DirectCaptureWake> = Arc::new(CountingWake(AtomicUsize::new(0)));
        let poll_thread = spawn_direct_capture_poll(
            buffer,
            device,
            submission_index,
            layout,
            DirectCaptureFormat::Rgba8Unorm,
            1,
            73,
            event_tx,
            wake,
        )
        .unwrap();

        assert!(matches!(
            event_rx.recv_timeout(Duration::from_secs(6)).unwrap(),
            DirectCaptureEvent::Mapped {
                token: 73,
                result: Ok(bytes),
            } if bytes.as_ref() == [1, 2, 3, 4]
        ));
        poll_thread.join().unwrap();
    }

    #[test]
    fn slipway_present_result_changes_only_after_surface_present() {
        let source = include_str!("winit.rs");
        let ordinary_start = source
            .find("fn paint_and_update_textures_impl<R: PaintResultReporter>")
            .unwrap();
        let ordinary_end = source[ordinary_start..]
            .find("pub fn paint_and_update_textures_with_direct_capture")
            .map(|offset| ordinary_start + offset)
            .unwrap();
        let ordinary = &source[ordinary_start..ordinary_end];
        assert_eq!(
            ordinary
                .matches("return R::finish(vsync_sec, false);")
                .count(),
            4
        );
        assert!(
            ordinary.find("output_frame.present();").unwrap()
                < ordinary.find("R::finish(vsync_sec, true)").unwrap()
        );

        let direct_start = ordinary_end;
        let direct_end = source[direct_start..]
            .find("/// Call this at the beginning of each frame")
            .map(|offset| direct_start + offset)
            .unwrap();
        let direct = &source[direct_start..direct_end];
        assert!(direct.contains("presented: false"));
        assert_eq!(direct.matches("paint_result.presented = true;").count(), 1);
        assert!(
            direct.find("output_frame.present();").unwrap()
                < direct.find("paint_result.presented = true;").unwrap()
        );
        assert!(
            direct.rfind("return paint_result;").unwrap()
                < direct.find("output_frame.present();").unwrap()
        );
    }

    #[test]
    fn direct_capture_callback_owns_no_buffer_or_external_sender() {
        let source = include_str!("winit.rs");
        let helper_start = source.find("fn spawn_direct_capture_poll(").unwrap();
        let callback_start = source[helper_start..]
            .find("move |result|")
            .map(|offset| helper_start + offset)
            .unwrap();
        let callback_end = source[callback_start..]
            .find("});")
            .map(|offset| callback_start + offset)
            .unwrap();
        let callback = &source[callback_start..callback_end];

        assert!(callback.contains("map_status_tx.send(result)"));
        for forbidden in [
            "buffer",
            "event_tx",
            "wake",
            "device",
            "token",
            "DirectCaptureEvent",
        ] {
            assert!(
                !callback.contains(forbidden),
                "callback retained {forbidden}"
            );
        }
    }

    #[test]
    fn direct_capture_poll_owner_emits_one_map_terminal() {
        let source = include_str!("winit.rs");
        let helper_start = source.find("fn spawn_direct_capture_poll(").unwrap();
        let helper_end = source[helper_start..]
            .find("struct SurfaceState")
            .map(|offset| helper_start + offset)
            .unwrap();
        let helper = &source[helper_start..helper_end];

        assert!(helper.contains("let mapped = buffer.slice(..).get_mapped_range();"));
        assert!(helper.contains("let event = match device.poll(wgpu::PollType::Wait"));
        assert!(helper.contains("DirectCaptureEvent::Mapped"));
        assert!(helper.contains("DirectCaptureEvent::PollFailed"));
        assert_eq!(helper.matches("send_direct_capture_event(").count(), 1);
        assert_eq!(helper.matches("device.poll(").count(), 1);
        assert!(!helper.contains("loop {"));
    }

    #[test]
    fn direct_surface_acquire_failure_sends_one_concrete_refusal_before_return() {
        let source = include_str!("winit.rs");
        let start = source
            .find("pub fn paint_and_update_textures_with_direct_capture")
            .unwrap();
        let end = source[start..]
            .find("/// Call this at the beginning of each frame")
            .map(|offset| start + offset)
            .unwrap();
        let method = &source[start..end];
        let refusal = method
            .find("DirectCaptureRefusal::SurfaceAcquireFailed")
            .unwrap();
        let following_return = method[refusal..].find("return paint_result;").unwrap();

        assert_eq!(
            method
                .matches("DirectCaptureRefusal::SurfaceAcquireFailed")
                .count(),
            1
        );
        assert!(following_return > 0);
    }

    #[test]
    fn enabled_idle_status_paint_has_zero_slipway_capture_work() {
        let source = include_str!("winit.rs");
        let start = source
            .find("pub fn paint_and_update_textures_with_slipway_present_result")
            .unwrap();
        let end = source[start..]
            .find("fn paint_and_update_textures_impl")
            .map(|offset| start + offset)
            .unwrap();
        let wrapper = &source[start..end];

        assert!(wrapper.contains("paint_and_update_textures_impl::<SlipwayDebugPresentReporter>"));
        for forbidden in [
            "sync_channel",
            "create_buffer",
            "map_async",
            ".spawn(",
            ".wake(",
        ] {
            assert!(
                !wrapper.contains(forbidden),
                "idle wrapper performs {forbidden}"
            );
        }
    }

    #[test]
    fn direct_capture_source_uses_one_presented_output_copy_and_no_framework_capture() {
        let source = include_str!("winit.rs");
        let start = source
            .find("pub fn paint_and_update_textures_with_direct_capture")
            .unwrap();
        let end = source[start..]
            .find("/// Call this at the beginning of each frame")
            .map(|offset| start + offset)
            .unwrap();
        let method = &source[start..end];

        assert!(!method.contains("CaptureState"));
        assert!(!method.contains("Event::Screenshot"));
        assert!(!method.contains("self.paint_and_update_textures("));
        assert_eq!(method.matches("copy_texture_to_buffer(").count(), 1);
        assert_eq!(method.matches("output_frame.present();").count(), 1);
        assert!(method.contains("output_frame.texture.as_image_copy()"));
        assert!(
            method.find("output_frame.present();").unwrap()
                < method.find("DirectCaptureEvent::Presented").unwrap()
        );
    }
}
