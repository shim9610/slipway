use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont, point};
use slipway_core::{
    BaselineShift, BoxSpacing, ClipDeclaration, Color, Diagnostic, DiagnosticSeverity,
    EvidenceSource, FontStyle, FontWeight, FrameIdentity, PREPARED_GEOMETRY_REQUIRED, PaintOp,
    PathCommand, PathDeclaration, PlacementBoxGeometry, PresentedBoxGeometry, Rect, RenderEvidence,
    RenderPacket, RenderRefusal, ShapeDeclaration, ShapeKind, Size, SlipwayOffscreenRenderer,
    TargetBoxGeometry, TextAlignX, TextAlignY, TextStyle, TextWrap, Translation, WidgetId,
    WidgetSlotAddress,
};
use std::fs::{self, File};
use std::io::BufWriter;

const DEFAULT_PROVIDER_ID: &str = "slipway-debug-renderer.cpu.v1";
const DEFAULT_MAX_PIXELS: u64 = 16_777_216;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugPngArtifactReceipt {
    pub artifact_ref: String,
    pub artifact_path: String,
    pub pixel_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebugPngArtifactError {
    ZeroSize,
    ByteLengthOverflow,
    ByteLengthMismatch { expected: usize, actual: usize },
    CreateDirectory(String),
    CreateFile(String),
    Encode(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugRendererConfig {
    pub provider_id: String,
    pub clear_color: Color,
    pub max_pixels: u64,
}

impl Default for DebugRendererConfig {
    fn default() -> Self {
        Self {
            provider_id: DEFAULT_PROVIDER_ID.to_string(),
            clear_color: Color {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 0.0,
            },
            max_pixels: DEFAULT_MAX_PIXELS,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugRenderArtifact {
    pub artifact_ref: String,
    pub artifact_path: Option<String>,
    pub target: WidgetId,
    pub frame: FrameIdentity,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub pixel_hash: String,
    pub box_geometry: Vec<DebugBoxGeometryEvidence>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugBoxGeometryEvidence {
    pub target: WidgetId,
    pub address: WidgetSlotAddress,
    pub spacing: BoxSpacing,
    pub target_local: TargetBoxGeometry,
    pub parent_local: PlacementBoxGeometry,
    pub unscrolled_root: PresentedBoxGeometry,
    pub scroll_translation: Translation,
    pub overlay_translation: Translation,
    pub final_presented: PresentedBoxGeometry,
    pub authored_overflow: Option<slipway_core::TargetLocalRect>,
    pub ancestor_clip_final: Option<Rect>,
    pub effective_clip_final: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactComparison {
    pub left_artifact_ref: String,
    pub right_artifact_ref: String,
    pub left_dimensions: (u32, u32),
    pub right_dimensions: (u32, u32),
    pub mismatch_pixel_count: u64,
    pub total_pixel_count: u64,
    pub exact_match: bool,
    pub normalized_difference: f32,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactLookupError {
    MissingArtifact { artifact_ref: String },
}

#[derive(Clone, Debug)]
pub struct CpuDebugRenderer {
    config: DebugRendererConfig,
    artifacts: Vec<DebugRenderArtifact>,
    fonts: DebugFontBook,
}

impl CpuDebugRenderer {
    pub fn new(config: DebugRendererConfig) -> Self {
        Self {
            config,
            artifacts: Vec::new(),
            fonts: DebugFontBook::from_system_fonts(),
        }
    }

    pub fn provider_id(&self) -> &str {
        &self.config.provider_id
    }

    pub fn artifacts(&self) -> &[DebugRenderArtifact] {
        &self.artifacts
    }

    pub fn artifact(&self, artifact_ref: &str) -> Option<&DebugRenderArtifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.artifact_ref == artifact_ref)
    }

    pub fn compare_artifacts(
        &self,
        left_artifact_ref: &str,
        right_artifact_ref: &str,
    ) -> Result<ArtifactComparison, ArtifactLookupError> {
        let left = self.artifact(left_artifact_ref).ok_or_else(|| {
            ArtifactLookupError::MissingArtifact {
                artifact_ref: left_artifact_ref.to_string(),
            }
        })?;
        let right = self.artifact(right_artifact_ref).ok_or_else(|| {
            ArtifactLookupError::MissingArtifact {
                artifact_ref: right_artifact_ref.to_string(),
            }
        })?;

        Ok(compare_artifact_bytes(left, right))
    }

    fn render_packet(&self, packet: &RenderPacket) -> Result<RenderedPixels, RenderRefusal> {
        let captured = packet
            .prepared_geometry
            .as_ref()
            .and_then(|prepared| prepared.captured_records())
            .ok_or_else(|| RenderRefusal {
                target: Some(packet.target.clone()),
                frame: packet.frame.clone(),
                source: Some(EvidenceSource::canonical_offscreen(
                    self.config.provider_id.clone(),
                )),
                provider_id: Some(self.config.provider_id.clone()),
                reason: "debug renderer requires core-prepared captured geometry".to_string(),
                diagnostics: vec![diagnostic(
                    Some(packet.target.clone()),
                    DiagnosticSeverity::Error,
                    PREPARED_GEOMETRY_REQUIRED,
                    "core-prepared captured geometry is required before canonical rasterization",
                )],
            })?;
        let box_geometry = captured
            .iter()
            .map(|record| DebugBoxGeometryEvidence {
                target: record.target.clone(),
                address: record.address.clone(),
                spacing: record.spacing,
                target_local: record.target_local,
                parent_local: record.parent_local,
                unscrolled_root: record.unscrolled_root,
                scroll_translation: record.scroll_translation,
                overlay_translation: record.overlay_translation,
                final_presented: record.final_presented,
                authored_overflow: record.authored_overflow,
                ancestor_clip_final: record.ancestor_clip_final,
                effective_clip_final: record.effective_clip_final,
            })
            .collect();
        let width = viewport_axis_to_pixels(packet.frame.viewport.size.width);
        let height = viewport_axis_to_pixels(packet.frame.viewport.size.height);
        let total_pixels = width as u64 * height as u64;
        if width == 0 || height == 0 {
            return Err(RenderRefusal {
                target: Some(packet.target.clone()),
                frame: packet.frame.clone(),
                source: Some(EvidenceSource::canonical_offscreen(
                    self.config.provider_id.clone(),
                )),
                provider_id: Some(self.config.provider_id.clone()),
                reason: "debug renderer requires a positive viewport".to_string(),
                diagnostics: packet.diagnostics.clone(),
            });
        }
        if total_pixels > self.config.max_pixels {
            return Err(RenderRefusal {
                target: Some(packet.target.clone()),
                frame: packet.frame.clone(),
                source: Some(EvidenceSource::canonical_offscreen(
                    self.config.provider_id.clone(),
                )),
                provider_id: Some(self.config.provider_id.clone()),
                reason: format!(
                    "debug renderer viewport has {total_pixels} pixels, above configured max {}",
                    self.config.max_pixels
                ),
                diagnostics: packet.diagnostics.clone(),
            });
        }

        let mut target = RasterTarget::new(width, height, self.config.clear_color);
        let mut diagnostics = packet.diagnostics.clone();

        if !packet.surfaces.is_empty() {
            diagnostics.push(diagnostic(
                Some(packet.target.clone()),
                DiagnosticSeverity::Unsupported,
                "render-surfaces-not-rasterized",
                "debug CPU renderer records render surface declarations but does not rasterize provider surfaces",
            ));
        }

        for op in &packet.paint {
            render_op(
                op,
                &packet.frame.viewport,
                &mut target,
                &mut diagnostics,
                &self.fonts,
            );
        }

        Ok(RenderedPixels {
            width,
            height,
            rgba: target.rgba,
            box_geometry,
            diagnostics,
        })
    }
}

impl Default for CpuDebugRenderer {
    fn default() -> Self {
        Self::new(DebugRendererConfig::default())
    }
}

impl SlipwayOffscreenRenderer for CpuDebugRenderer {
    fn render_offscreen(&mut self, packet: RenderPacket) -> Result<RenderEvidence, RenderRefusal> {
        let rendered = self.render_packet(&packet)?;
        let mut diagnostics = rendered.diagnostics;
        let receipt = write_debug_rgba8_png_artifact(
            &self.config.provider_id,
            packet.target.as_str(),
            &packet.frame,
            rendered.width,
            rendered.height,
            &rendered.rgba,
        );
        let (artifact_ref, artifact_path, pixel_hash) = match receipt {
            Ok(receipt) => (
                receipt.artifact_ref,
                Some(receipt.artifact_path),
                receipt.pixel_hash,
            ),
            Err(error) => {
                diagnostics.push(diagnostic(
                    Some(packet.target.clone()),
                    DiagnosticSeverity::Warning,
                    "debug-render-png-artifact-write-failed",
                    format!("debug renderer could not write PNG artifact: {error:?}"),
                ));
                let pixel_hash = pixel_hash(rendered.width, rendered.height, &rendered.rgba);
                let artifact_ref = artifact_ref(
                    &self.config.provider_id,
                    packet.target.as_str(),
                    &packet.frame,
                    &pixel_hash,
                );
                (artifact_ref, None, pixel_hash)
            }
        };
        let artifact = DebugRenderArtifact {
            artifact_ref: artifact_ref.clone(),
            artifact_path: artifact_path.clone(),
            target: packet.target.clone(),
            frame: packet.frame.clone(),
            width: rendered.width,
            height: rendered.height,
            rgba: rendered.rgba,
            pixel_hash: pixel_hash.clone(),
            box_geometry: rendered.box_geometry,
            diagnostics: diagnostics.clone(),
        };

        if let Some(existing) = self
            .artifacts
            .iter_mut()
            .find(|existing| existing.artifact_ref == artifact.artifact_ref)
        {
            *existing = artifact;
        } else {
            self.artifacts.push(artifact);
        }

        Ok(RenderEvidence {
            target: packet.target,
            frame: packet.frame,
            source: EvidenceSource::canonical_offscreen(self.config.provider_id.clone()),
            provider_id: self.config.provider_id.clone(),
            artifact_ref: Some(artifact_ref),
            artifact_path,
            pixel_hash: Some(pixel_hash),
            width: Some(rendered.width),
            height: Some(rendered.height),
            diagnostics,
        })
    }
}

struct RenderedPixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    box_geometry: Vec<DebugBoxGeometryEvidence>,
    diagnostics: Vec<Diagnostic>,
}

struct RasterTarget {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    clip_stack: Vec<PixelBounds>,
}

impl RasterTarget {
    fn new(width: u32, height: u32, clear_color: Color) -> Self {
        let clear = color_to_rgba(clear_color);
        let mut rgba = vec![0; width as usize * height as usize * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&clear);
        }
        Self {
            width,
            height,
            rgba,
            clip_stack: Vec::new(),
        }
    }

    fn blend_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        if self
            .clip_stack
            .last()
            .is_some_and(|clip| !clip.contains(x, y))
        {
            return;
        }
        let offset = ((y as u32 * self.width + x as u32) * 4) as usize;
        let source = color_to_rgba(color);
        blend_rgba(&mut self.rgba[offset..offset + 4], source);
    }

    fn with_clip<R>(&mut self, clip: PixelBounds, f: impl FnOnce(&mut Self) -> R) -> R {
        let effective = self
            .clip_stack
            .last()
            .copied()
            .map(|current| current.intersect(clip))
            .unwrap_or(clip);
        self.clip_stack.push(effective);
        let result = f(self);
        self.clip_stack.pop();
        result
    }
}

fn render_op(
    op: &PaintOp,
    viewport: &Rect,
    target: &mut RasterTarget,
    diagnostics: &mut Vec<Diagnostic>,
    fonts: &DebugFontBook,
) {
    match op {
        PaintOp::Fill { shape, color } => {
            with_optional_clip(shape.clip.as_ref(), viewport, target, |target| {
                fill_shape(shape, *color, viewport, target, diagnostics, fonts)
            })
        }
        PaintOp::Stroke {
            shape,
            color,
            width,
        } => with_optional_clip(shape.clip.as_ref(), viewport, target, |target| {
            stroke_shape(shape, *color, *width, viewport, target, diagnostics, fonts)
        }),
        PaintOp::Text {
            bounds,
            content,
            color,
            style,
        } => draw_text_glyphs(
            *bounds,
            content,
            *color,
            style,
            viewport,
            target,
            diagnostics,
            fonts,
        ),
        PaintOp::Group { clip, ops, .. } | PaintOp::Layer { clip, ops, .. } => {
            with_optional_clip(clip.as_ref(), viewport, target, |target| {
                for child in ops {
                    render_op(child, viewport, target, diagnostics, fonts);
                }
            });
        }
    }
}

fn with_optional_clip<R>(
    clip: Option<&ClipDeclaration>,
    viewport: &Rect,
    target: &mut RasterTarget,
    f: impl FnOnce(&mut RasterTarget) -> R,
) -> R {
    if let Some(clip) = clip {
        let bounds = pixel_bounds(clip.bounds, viewport).clipped(target);
        target.with_clip(bounds, f)
    } else {
        f(target)
    }
}

fn fill_shape(
    shape: &ShapeDeclaration,
    color: Color,
    viewport: &Rect,
    target: &mut RasterTarget,
    diagnostics: &mut Vec<Diagnostic>,
    fonts: &DebugFontBook,
) {
    match shape.kind {
        ShapeKind::Rectangle => fill_rect(shape.bounds, color, viewport, target),
        ShapeKind::RoundedRectangle => {
            diagnostics.push(approximation_diagnostic(
                shape,
                "rounded-rectangle-fill-approximated",
                "rounded rectangle fill is rasterized as its bounding rectangle",
            ));
            fill_rect(shape.bounds, color, viewport, target);
        }
        ShapeKind::Circle => fill_circle(shape.bounds, color, viewport, target),
        ShapeKind::Line => {
            stroke_line(shape.bounds, color, 1.0, viewport, target);
            diagnostics.push(approximation_diagnostic(
                shape,
                "line-fill-approximated",
                "line fill is rasterized as a one-pixel stroke",
            ));
        }
        ShapeKind::Path => fill_path_shape(shape, color, viewport, target, diagnostics),
        ShapeKind::Text => {
            diagnostics.push(approximation_diagnostic(
                shape,
                "text-shape-fill-approximated",
                "text shape fill is rasterized as placeholder marks",
            ));
            draw_text_glyphs(
                shape.bounds,
                shape.id.as_deref().unwrap_or("text"),
                color,
                &TextStyle::plain(),
                viewport,
                target,
                diagnostics,
                fonts,
            );
        }
    }
}

fn stroke_shape(
    shape: &ShapeDeclaration,
    color: Color,
    width: f32,
    viewport: &Rect,
    target: &mut RasterTarget,
    diagnostics: &mut Vec<Diagnostic>,
    fonts: &DebugFontBook,
) {
    let width = width.max(1.0);
    match shape.kind {
        ShapeKind::Rectangle => stroke_rect(shape.bounds, color, width, viewport, target),
        ShapeKind::RoundedRectangle => {
            diagnostics.push(approximation_diagnostic(
                shape,
                "rounded-rectangle-stroke-approximated",
                "rounded rectangle stroke is rasterized as its bounding rectangle stroke",
            ));
            stroke_rect(shape.bounds, color, width, viewport, target);
        }
        ShapeKind::Circle => stroke_circle(shape.bounds, color, width, viewport, target),
        ShapeKind::Line => stroke_line(shape.bounds, color, width, viewport, target),
        ShapeKind::Path => stroke_path_shape(shape, color, width, viewport, target, diagnostics),
        ShapeKind::Text => {
            diagnostics.push(approximation_diagnostic(
                shape,
                "text-shape-stroke-approximated",
                "text shape stroke is rasterized as placeholder marks",
            ));
            draw_text_glyphs(
                shape.bounds,
                shape.id.as_deref().unwrap_or("text"),
                color,
                &TextStyle::plain(),
                viewport,
                target,
                diagnostics,
                fonts,
            );
        }
    }
}

fn fill_path_shape(
    shape: &ShapeDeclaration,
    color: Color,
    viewport: &Rect,
    target: &mut RasterTarget,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(approximation_diagnostic(
        shape,
        "path-fill-approximated",
        "path fill is rasterized as its path outline by the canonical offscreen debug renderer",
    ));
    stroke_path_shape(shape, color, 1.0, viewport, target, diagnostics);
}

fn stroke_path_shape(
    shape: &ShapeDeclaration,
    color: Color,
    width: f32,
    viewport: &Rect,
    target: &mut RasterTarget,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(path) = shape.path.as_ref() else {
        diagnostics.push(diagnostic(
            shape.id.as_ref().map(|id| WidgetId::from(id.clone())),
            DiagnosticSeverity::Unsupported,
            "path-shape-missing-path-data",
            "path shape cannot be rasterized because it has no path declaration",
        ));
        return;
    };

    let had_curves = path.commands.iter().any(|command| {
        matches!(
            command,
            PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
        )
    });
    if had_curves {
        diagnostics.push(approximation_diagnostic(
            shape,
            "path-curve-stroke-approximated",
            "path curves are rasterized as line segments by the canonical offscreen debug renderer",
        ));
    }

    if !stroke_path(path, color, width, viewport, target) {
        diagnostics.push(diagnostic(
            shape.id.as_ref().map(|id| WidgetId::from(id.clone())),
            DiagnosticSeverity::Unsupported,
            "path-shape-empty",
            "path shape contains no drawable segments",
        ));
    }
}

fn stroke_path(
    path: &PathDeclaration,
    color: Color,
    width: f32,
    viewport: &Rect,
    target: &mut RasterTarget,
) -> bool {
    let mut current: Option<slipway_core::Point> = None;
    let mut subpath_start: Option<slipway_core::Point> = None;
    let mut drew = false;

    for command in &path.commands {
        match command {
            PathCommand::MoveTo(point) => {
                current = Some(*point);
                subpath_start = Some(*point);
            }
            PathCommand::LineTo(to) => {
                if let Some(from) = current {
                    stroke_point_segment(from, *to, color, width, viewport, target);
                    drew = true;
                }
                current = Some(*to);
            }
            PathCommand::QuadraticTo { control, to } => {
                if let Some(from) = current {
                    stroke_quadratic_segments(from, *control, *to, color, width, viewport, target);
                    drew = true;
                }
                current = Some(*to);
            }
            PathCommand::CubicTo {
                control_1,
                control_2,
                to,
            } => {
                if let Some(from) = current {
                    stroke_cubic_segments(
                        from, *control_1, *control_2, *to, color, width, viewport, target,
                    );
                    drew = true;
                }
                current = Some(*to);
            }
            PathCommand::Close => {
                if let (Some(from), Some(to)) = (current, subpath_start) {
                    stroke_point_segment(from, to, color, width, viewport, target);
                    current = Some(to);
                    drew = true;
                }
            }
        }
    }

    drew
}

fn stroke_quadratic_segments(
    from: slipway_core::Point,
    control: slipway_core::Point,
    to: slipway_core::Point,
    color: Color,
    width: f32,
    viewport: &Rect,
    target: &mut RasterTarget,
) {
    let mut previous = from;
    for step in 1..=16 {
        let t = step as f32 / 16.0;
        let next = quadratic_point(from, control, to, t);
        stroke_point_segment(previous, next, color, width, viewport, target);
        previous = next;
    }
}

fn stroke_cubic_segments(
    from: slipway_core::Point,
    control_1: slipway_core::Point,
    control_2: slipway_core::Point,
    to: slipway_core::Point,
    color: Color,
    width: f32,
    viewport: &Rect,
    target: &mut RasterTarget,
) {
    let mut previous = from;
    for step in 1..=24 {
        let t = step as f32 / 24.0;
        let next = cubic_point(from, control_1, control_2, to, t);
        stroke_point_segment(previous, next, color, width, viewport, target);
        previous = next;
    }
}

fn quadratic_point(
    from: slipway_core::Point,
    control: slipway_core::Point,
    to: slipway_core::Point,
    t: f32,
) -> slipway_core::Point {
    let inv = 1.0 - t;
    slipway_core::Point {
        x: inv * inv * from.x + 2.0 * inv * t * control.x + t * t * to.x,
        y: inv * inv * from.y + 2.0 * inv * t * control.y + t * t * to.y,
    }
}

fn cubic_point(
    from: slipway_core::Point,
    control_1: slipway_core::Point,
    control_2: slipway_core::Point,
    to: slipway_core::Point,
    t: f32,
) -> slipway_core::Point {
    let inv = 1.0 - t;
    slipway_core::Point {
        x: inv * inv * inv * from.x
            + 3.0 * inv * inv * t * control_1.x
            + 3.0 * inv * t * t * control_2.x
            + t * t * t * to.x,
        y: inv * inv * inv * from.y
            + 3.0 * inv * inv * t * control_1.y
            + 3.0 * inv * t * t * control_2.y
            + t * t * t * to.y,
    }
}

fn stroke_point_segment(
    from: slipway_core::Point,
    to: slipway_core::Point,
    color: Color,
    width: f32,
    viewport: &Rect,
    target: &mut RasterTarget,
) {
    stroke_line(
        Rect {
            origin: from,
            size: Size {
                width: to.x - from.x,
                height: to.y - from.y,
            },
        },
        color,
        width,
        viewport,
        target,
    );
}

fn fill_rect(rect: Rect, color: Color, viewport: &Rect, target: &mut RasterTarget) {
    let bounds = pixel_bounds(rect, viewport).clipped(target);
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            target.blend_pixel(x, y, color);
        }
    }
}

fn stroke_rect(rect: Rect, color: Color, width: f32, viewport: &Rect, target: &mut RasterTarget) {
    let half = width.max(1.0).ceil() as i32;
    let raw_bounds = pixel_bounds(rect, viewport);
    let bounds = raw_bounds.clipped(target);
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            let near_left = x - raw_bounds.x0 < half;
            let near_right = raw_bounds.x1 - x <= half;
            let near_top = y - raw_bounds.y0 < half;
            let near_bottom = raw_bounds.y1 - y <= half;
            if near_left || near_right || near_top || near_bottom {
                target.blend_pixel(x, y, color);
            }
        }
    }
}

fn fill_circle(rect: Rect, color: Color, viewport: &Rect, target: &mut RasterTarget) {
    let bounds = pixel_bounds(rect, viewport).clipped(target);
    let cx = rect.origin.x + rect.size.width / 2.0 - viewport.origin.x;
    let cy = rect.origin.y + rect.size.height / 2.0 - viewport.origin.y;
    let rx = (rect.size.width / 2.0).max(0.5);
    let ry = (rect.size.height / 2.0).max(0.5);
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let nx = (px - cx) / rx;
            let ny = (py - cy) / ry;
            if nx * nx + ny * ny <= 1.0 {
                target.blend_pixel(x, y, color);
            }
        }
    }
}

fn stroke_circle(rect: Rect, color: Color, width: f32, viewport: &Rect, target: &mut RasterTarget) {
    let bounds = pixel_bounds(rect, viewport).clipped(target);
    let cx = rect.origin.x + rect.size.width / 2.0 - viewport.origin.x;
    let cy = rect.origin.y + rect.size.height / 2.0 - viewport.origin.y;
    let rx = (rect.size.width / 2.0).max(0.5);
    let ry = (rect.size.height / 2.0).max(0.5);
    let threshold = (width / rx.min(ry).max(1.0)).max(0.02);
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let nx = (px - cx) / rx;
            let ny = (py - cy) / ry;
            let distance = (nx * nx + ny * ny).sqrt();
            if (1.0 - distance).abs() <= threshold {
                target.blend_pixel(x, y, color);
            }
        }
    }
}

fn stroke_line(rect: Rect, color: Color, width: f32, viewport: &Rect, target: &mut RasterTarget) {
    let x0 = rect.origin.x - viewport.origin.x;
    let y0 = rect.origin.y - viewport.origin.y;
    let x1 = rect.origin.x + rect.size.width - viewport.origin.x;
    let y1 = rect.origin.y + rect.size.height - viewport.origin.y;
    let min_x = x0.min(x1) - width;
    let max_x = x0.max(x1) + width;
    let min_y = y0.min(y1) - width;
    let max_y = y0.max(y1) + width;
    let bounds = PixelBounds {
        x0: min_x.floor() as i32,
        y0: min_y.floor() as i32,
        x1: max_x.ceil() as i32,
        y1: max_y.ceil() as i32,
    }
    .clipped(target);
    let threshold = width.max(1.0) / 2.0;
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            let distance = distance_to_segment(x as f32 + 0.5, y as f32 + 0.5, x0, y0, x1, y1);
            if distance <= threshold {
                target.blend_pixel(x, y, color);
            }
        }
    }
}

#[derive(Clone, Debug)]
struct DebugFontBook {
    regular: Option<DebugFontFace>,
    bold: Option<DebugFontFace>,
    italic: Option<DebugFontFace>,
}

#[derive(Clone, Debug)]
struct DebugFontFace {
    family: String,
    font: FontArc,
}

impl DebugFontBook {
    fn from_system_fonts() -> Self {
        Self {
            regular: load_first_font(&[
                r"C:\Windows\Fonts\malgun.ttf",
                r"C:\Windows\Fonts\segoeui.ttf",
                r"C:\Windows\Fonts\arial.ttf",
            ]),
            bold: load_first_font(&[
                r"C:\Windows\Fonts\malgunbd.ttf",
                r"C:\Windows\Fonts\segoeuib.ttf",
                r"C:\Windows\Fonts\arialbd.ttf",
            ]),
            italic: load_first_font(&[
                r"C:\Windows\Fonts\malgunsl.ttf",
                r"C:\Windows\Fonts\segoeuii.ttf",
                r"C:\Windows\Fonts\ariali.ttf",
            ]),
        }
    }

    fn face(&self, style: &TextStyle) -> Option<&DebugFontFace> {
        let is_bold = font_weight_value(style.font_weight) >= 600;
        let is_italic = style.font_style == FontStyle::Italic;
        if is_bold {
            self.bold
                .as_ref()
                .or(self.italic.as_ref().filter(|_| is_italic))
                .or(self.regular.as_ref())
        } else if is_italic {
            self.italic.as_ref().or(self.regular.as_ref())
        } else {
            self.regular.as_ref()
        }
    }
}

fn load_first_font(paths: &[&str]) -> Option<DebugFontFace> {
    paths.iter().find_map(|path| {
        fs::read(path)
            .ok()
            .and_then(|bytes| FontArc::try_from_vec(bytes).ok())
            .map(|font| DebugFontFace {
                family: (*path).to_string(),
                font,
            })
    })
}

/// One laid-out text line: the characters to draw and the measured ink
/// width (trailing whitespace excluded) used for the per-line
/// [`TextAlignX`] anchor.
struct DebugTextLine {
    text: String,
    width: f32,
}

fn measure_debug_line<F, SF>(scaled: &SF, text: &str) -> f32
where
    F: Font,
    SF: ScaleFont<F>,
{
    let mut width = 0.0f32;
    let mut previous: Option<GlyphId> = None;
    for ch in text.trim_end().chars() {
        if ch.is_control() {
            continue;
        }
        let glyph_id = scaled.glyph_id(ch);
        if let Some(previous_id) = previous {
            width += scaled.kern(previous_id, glyph_id);
        }
        previous = Some(glyph_id);
        width += scaled.h_advance(glyph_id);
    }
    width
}

/// Lays `content` out into lines the same way the visible backends do:
/// explicit `\n` always breaks (both wrap modes); [`TextWrap::Word`]
/// additionally soft-wraps greedily at whitespace against `max_width`
/// (a single word wider than the rect stays on its own overflowing line
/// and clips — the cosmic-text `Wrap::Word` rule iced presents);
/// [`TextWrap::None`] never soft-wraps.
fn layout_debug_text_lines<F, SF>(
    scaled: &SF,
    content: &str,
    wrap: TextWrap,
    max_width: f32,
) -> Vec<DebugTextLine>
where
    F: Font,
    SF: ScaleFont<F>,
{
    let mut lines = Vec::new();
    for hard_line in content.split('\n') {
        match wrap {
            TextWrap::None => lines.push(DebugTextLine {
                text: hard_line.to_string(),
                width: measure_debug_line(scaled, hard_line),
            }),
            TextWrap::Word => {
                let mut current = String::new();
                let mut chars = hard_line.chars().peekable();
                while chars.peek().is_some() {
                    let mut token = String::new();
                    let token_is_space = chars.peek().is_some_and(|ch| ch.is_whitespace());
                    while chars
                        .peek()
                        .is_some_and(|ch| ch.is_whitespace() == token_is_space)
                    {
                        token.push(chars.next().expect("peeked char"));
                    }
                    if token_is_space {
                        // Whitespace never starts a soft-wrapped line.
                        if !current.is_empty() {
                            current.push_str(&token);
                        }
                        continue;
                    }
                    let has_word = !current.trim_end().is_empty();
                    let candidate = format!("{current}{token}");
                    if has_word && measure_debug_line(scaled, &candidate) > max_width {
                        lines.push(DebugTextLine {
                            width: measure_debug_line(scaled, &current),
                            text: current.trim_end().to_string(),
                        });
                        current = token;
                    } else {
                        current = candidate;
                    }
                }
                lines.push(DebugTextLine {
                    width: measure_debug_line(scaled, &current),
                    text: current.trim_end().to_string(),
                });
            }
        }
    }
    lines
}

fn draw_text_glyphs(
    bounds: Rect,
    content: &str,
    color: Color,
    style: &TextStyle,
    viewport: &Rect,
    target: &mut RasterTarget,
    diagnostics: &mut Vec<Diagnostic>,
    fonts: &DebugFontBook,
) {
    // Layout/anchor math uses the UNCLIPPED op rect (the declared bounds
    // in pixel space, matching the visible backends' anchor rule);
    // drawing clips to the target-intersected rect.
    let raw_bounds = pixel_bounds(bounds, viewport);
    let clip = raw_bounds.clipped(target);
    if clip.x0 >= clip.x1 || clip.y0 >= clip.y1 {
        return;
    }

    let Some(face) = fonts.face(style) else {
        diagnostics.push(diagnostic(
            None,
            DiagnosticSeverity::Warning,
            "debug-text-font-unavailable",
            format!(
                "debug renderer could not load a real system font for TextStyle {}; placeholder marks were used",
                text_style_summary(style)
            ),
        ));
        draw_text_placeholder_fallback(clip, content, color, style, target);
        return;
    };

    let font_size = normalized_font_size(style);
    let scale = PxScale::from(font_size);
    let scaled = face.font.as_scaled(scale);
    let baseline_offset = baseline_pixel_offset(style.baseline, font_size) as f32;
    let line_height = (scaled.ascent() - scaled.descent() + scaled.line_gap()).max(font_size);
    let rect_width = (raw_bounds.x1 - raw_bounds.x0) as f32;
    let rect_height = (raw_bounds.y1 - raw_bounds.y0) as f32;

    // Wrap first (at the rect width, per the declared mode), then anchor
    // the wrapped BLOCK vertically and each line horizontally — the same
    // order of operations both visible backends apply
    // (docs/public/api/backends.md "Text Wrap and Alignment").
    let lines = layout_debug_text_lines(&scaled, content, style.wrap, rect_width);
    let block_height = lines.len() as f32 * line_height;
    let block_top = raw_bounds.y0 as f32
        + match style.align_y {
            TextAlignY::Top => 0.0,
            TextAlignY::Center => (rect_height - block_height) / 2.0,
            TextAlignY::Bottom => rect_height - block_height,
        };

    let mut drawn_any = false;
    let mut missing = 0usize;

    for (line_index, line) in lines.iter().enumerate() {
        let line_top = block_top + line_index as f32 * line_height;
        if line_top > clip.y1 as f32 {
            break;
        }
        if line_top + line_height < clip.y0 as f32 {
            continue;
        }
        let line_start = raw_bounds.x0 as f32
            + match style.align_x {
                TextAlignX::Start => 0.0,
                TextAlignX::Center => (rect_width - line.width) / 2.0,
                TextAlignX::End => rect_width - line.width,
            };
        let baseline_y = line_top + scaled.ascent() + baseline_offset;
        let mut caret = point(line_start, baseline_y);
        let mut previous: Option<GlyphId> = None;

        for ch in line.text.chars() {
            if ch.is_control() {
                continue;
            }
            let glyph_id = scaled.glyph_id(ch);
            if let Some(previous_id) = previous {
                caret.x += scaled.kern(previous_id, glyph_id);
            }
            previous = Some(glyph_id);

            let advance = scaled.h_advance(glyph_id);
            if ch.is_whitespace() {
                caret.x += advance;
                continue;
            }
            if caret.x > clip.x1 as f32 {
                break;
            }

            let glyph = glyph_id.with_scale_and_position(scale, caret);
            if let Some(outlined) = scaled.outline_glyph(glyph) {
                let glyph_bounds = outlined.px_bounds();
                outlined.draw(|x, y, coverage| {
                    blend_coverage_pixel(
                        target,
                        clip,
                        glyph_bounds.min.x as i32 + x as i32,
                        glyph_bounds.min.y as i32 + y as i32,
                        color,
                        coverage,
                    );
                });
                drawn_any = true;
            } else {
                missing += 1;
            }
            caret.x += advance;
        }

        // Decorations follow the REAL laid-out line geometry (start,
        // width, baseline) so they track alignment and wrap.
        if line.width > 0.0 {
            let weight_pixels = font_weight_pixels(style.font_weight);
            if style.decoration.underline {
                draw_horizontal_text_line(
                    clip,
                    color,
                    target,
                    line_start.round() as i32,
                    (baseline_y + 1.0).round() as i32,
                    line.width.round() as i32,
                    weight_pixels,
                );
            }
            if style.decoration.strikethrough {
                draw_horizontal_text_line(
                    clip,
                    color,
                    target,
                    line_start.round() as i32,
                    (baseline_y - scaled.ascent() * 0.33).round() as i32,
                    line.width.round() as i32,
                    weight_pixels,
                );
            }
        }
    }

    if missing > 0 {
        diagnostics.push(diagnostic(
            None,
            DiagnosticSeverity::Warning,
            "debug-text-glyphs-missing",
            format!(
                "debug renderer font '{}' could not outline {missing} glyph(s) for TextStyle {}",
                face.family,
                text_style_summary(style)
            ),
        ));
    }
    if !drawn_any && !content.trim().is_empty() {
        diagnostics.push(diagnostic(
            None,
            DiagnosticSeverity::Warning,
            "debug-text-no-glyphs-drawn",
            format!(
                "debug renderer loaded font '{}' but drew no glyph pixels for TextStyle {}",
                face.family,
                text_style_summary(style)
            ),
        ));
    }
}

fn draw_text_placeholder_fallback(
    bounds: PixelBounds,
    content: &str,
    color: Color,
    style: &TextStyle,
    target: &mut RasterTarget,
) {
    let font_size = normalized_font_size(style);
    let scale = (font_size / slipway_core::DEFAULT_TEXT_FONT_SIZE).clamp(0.5, 4.0);
    let weight_pixels = font_weight_pixels(style.font_weight);
    let char_width = ((6.0 * scale).round() as i32).max(weight_pixels + 2);
    let char_height = ((9.0 * scale).round() as i32).max(weight_pixels + 3);
    let mark_width = ((4.0 * scale).round() as i32).max(weight_pixels + 1);
    let mark_height = ((7.0 * scale).round() as i32).max(weight_pixels + 2);
    let available_width = (bounds.x1 - bounds.x0).max(char_width);
    let chars_per_line = (available_width / char_width).max(1);
    let baseline_offset = baseline_pixel_offset(style.baseline, font_size);
    let style_seed = text_style_seed(style);
    let italic = style.font_style == FontStyle::Italic;

    for (index, ch) in content.chars().enumerate() {
        if ch.is_whitespace() {
            continue;
        }
        let column = index as i32 % chars_per_line;
        let row = index as i32 / chars_per_line;
        let origin_x = bounds.x0 + column * char_width;
        let origin_y = bounds.y0 + row * char_height + baseline_offset;
        if origin_y >= bounds.y1 {
            break;
        }
        let seed = ch as u32 ^ ((index as u32 + 1) * 0x45d9_f3b) ^ style_seed;
        for y in 0..mark_height {
            let italic_offset = if italic { (mark_height - y - 1) / 3 } else { 0 };
            for x in 0..mark_width {
                let bit = ((seed.rotate_left((x + y * mark_width) as u32) >> 3) ^ seed) & 1;
                let border = x == 0 || y == 0 || x == mark_width - 1 || y == mark_height - 1;
                if bit == 1 || border {
                    let px = origin_x + x + italic_offset;
                    let py = origin_y + y;
                    blend_weighted_pixel(target, bounds, px, py, color, weight_pixels);
                }
            }
        }
    }

    draw_text_decorations(
        bounds,
        content,
        color,
        style,
        target,
        char_width,
        char_height,
    );
}

fn draw_text_decorations(
    bounds: PixelBounds,
    content: &str,
    color: Color,
    style: &TextStyle,
    target: &mut RasterTarget,
    char_width: i32,
    char_height: i32,
) {
    if !style.decoration.underline && !style.decoration.strikethrough {
        return;
    }

    let content_len = content.chars().count() as i32;
    let available_width = (bounds.x1 - bounds.x0).max(char_width);
    let chars_per_line = (available_width / char_width).max(1);
    let rows = ((content_len + chars_per_line - 1) / chars_per_line).max(1);
    let max_rows = ((bounds.y1 - bounds.y0 + char_height - 1) / char_height).max(1);
    let rows = rows.min(max_rows);
    let weight_pixels = font_weight_pixels(style.font_weight);

    for row in 0..rows {
        let row_y = bounds.y0 + row * char_height;
        let line_width = if row == rows - 1 {
            let remaining = content_len - row * chars_per_line;
            (remaining.max(1) * char_width).min(available_width)
        } else {
            available_width
        };

        if style.decoration.underline {
            let y = row_y + char_height - weight_pixels.max(1);
            draw_horizontal_text_line(
                bounds,
                color,
                target,
                bounds.x0,
                y,
                line_width,
                weight_pixels,
            );
        }

        if style.decoration.strikethrough {
            let y = row_y + char_height / 2;
            draw_horizontal_text_line(
                bounds,
                color,
                target,
                bounds.x0,
                y,
                line_width,
                weight_pixels,
            );
        }
    }
}

fn draw_horizontal_text_line(
    bounds: PixelBounds,
    color: Color,
    target: &mut RasterTarget,
    x0: i32,
    y: i32,
    width: i32,
    thickness: i32,
) {
    for dy in 0..thickness.max(1) {
        for x in x0..(x0 + width.max(1)) {
            blend_weighted_pixel(target, bounds, x, y + dy, color, 1);
        }
    }
}

fn blend_weighted_pixel(
    target: &mut RasterTarget,
    bounds: PixelBounds,
    x: i32,
    y: i32,
    color: Color,
    thickness: i32,
) {
    for dy in 0..thickness.max(1) {
        for dx in 0..thickness.max(1) {
            let px = x + dx;
            let py = y + dy;
            if px >= bounds.x0 && px < bounds.x1 && py >= bounds.y0 && py < bounds.y1 {
                target.blend_pixel(px, py, color);
            }
        }
    }
}

fn blend_coverage_pixel(
    target: &mut RasterTarget,
    bounds: PixelBounds,
    x: i32,
    y: i32,
    color: Color,
    coverage: f32,
) {
    if x < bounds.x0 || x >= bounds.x1 || y < bounds.y0 || y >= bounds.y1 {
        return;
    }
    if x < 0 || y < 0 || x >= target.width as i32 || y >= target.height as i32 {
        return;
    }

    let offset = ((y as u32 * target.width + x as u32) * 4) as usize;
    let mut source = color_to_rgba(color);
    source[3] = channel_to_u8((source[3] as f32 / 255.0) * coverage.clamp(0.0, 1.0));
    blend_rgba(&mut target.rgba[offset..offset + 4], source);
}

fn normalized_font_size(style: &TextStyle) -> f32 {
    if style.font_size.is_finite() {
        style.font_size.max(1.0)
    } else {
        slipway_core::DEFAULT_TEXT_FONT_SIZE
    }
}

fn font_weight_value(weight: FontWeight) -> u16 {
    match weight {
        FontWeight::Normal => 400,
        FontWeight::Bold => 700,
        FontWeight::Weight(value) => value.clamp(1, 1000),
    }
}

fn font_weight_pixels(weight: FontWeight) -> i32 {
    match font_weight_value(weight) {
        0..=599 => 1,
        600..=849 => 2,
        _ => 3,
    }
}

fn baseline_pixel_offset(baseline: BaselineShift, font_size: f32) -> i32 {
    match baseline {
        BaselineShift::Normal => 0,
        BaselineShift::Superscript => (font_size * -0.35).round() as i32,
        BaselineShift::Subscript => (font_size * 0.25).round() as i32,
    }
}

fn text_style_seed(style: &TextStyle) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in style.font_family.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    for byte in normalized_font_size(style).to_bits().to_le_bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash ^= font_weight_value(style.font_weight) as u32;
    hash = hash.wrapping_mul(0x0100_0193);
    hash ^= match style.font_style {
        FontStyle::Normal => 0,
        FontStyle::Italic => 1,
    };
    hash = hash.wrapping_mul(0x0100_0193);
    hash ^= (style.decoration.underline as u32) << 1;
    hash ^= (style.decoration.strikethrough as u32) << 2;
    hash = hash.wrapping_mul(0x0100_0193);
    hash ^ match style.baseline {
        BaselineShift::Normal => 0,
        BaselineShift::Superscript => 3,
        BaselineShift::Subscript => 5,
    }
}

fn text_style_summary(style: &TextStyle) -> String {
    format!(
        "family='{}', size={}, weight={}, style={:?}, underline={}, strikethrough={}, baseline={:?}",
        style.font_family,
        normalized_font_size(style),
        font_weight_value(style.font_weight),
        style.font_style,
        style.decoration.underline,
        style.decoration.strikethrough,
        style.baseline
    )
}

#[derive(Clone, Copy)]
struct PixelBounds {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl PixelBounds {
    fn clipped(self, target: &RasterTarget) -> Self {
        Self {
            x0: self.x0.clamp(0, target.width as i32),
            y0: self.y0.clamp(0, target.height as i32),
            x1: self.x1.clamp(0, target.width as i32),
            y1: self.y1.clamp(0, target.height as i32),
        }
    }

    fn intersect(self, other: Self) -> Self {
        Self {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

fn pixel_bounds(rect: Rect, viewport: &Rect) -> PixelBounds {
    PixelBounds {
        x0: (rect.origin.x - viewport.origin.x).floor() as i32,
        y0: (rect.origin.y - viewport.origin.y).floor() as i32,
        x1: (rect.origin.x + rect.size.width - viewport.origin.x).ceil() as i32,
        y1: (rect.origin.y + rect.size.height - viewport.origin.y).ceil() as i32,
    }
}

fn viewport_axis_to_pixels(value: f32) -> u32 {
    if value.is_finite() && value > 0.0 {
        value.ceil() as u32
    } else {
        0
    }
}

fn distance_to_segment(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return ((px - x0).powi(2) + (py - y0).powi(2)).sqrt();
    }
    let t = (((px - x0) * dx + (py - y0) * dy) / length_squared).clamp(0.0, 1.0);
    let closest_x = x0 + t * dx;
    let closest_y = y0 + t * dy;
    ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt()
}

fn color_to_rgba(color: Color) -> [u8; 4] {
    [
        channel_to_u8(color.red),
        channel_to_u8(color.green),
        channel_to_u8(color.blue),
        channel_to_u8(color.alpha),
    ]
}

fn channel_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn blend_rgba(destination: &mut [u8], source: [u8; 4]) {
    let sa = source[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }
    let da = destination[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        destination.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }

    for channel in 0..3 {
        let sc = source[channel] as f32 / 255.0;
        let dc = destination[channel] as f32 / 255.0;
        let out = (sc * sa + dc * da * (1.0 - sa)) / out_a;
        destination[channel] = channel_to_u8(out);
    }
    destination[3] = channel_to_u8(out_a);
}

fn pixel_hash(width: u32, height: u32, rgba: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in width
        .to_le_bytes()
        .into_iter()
        .chain(height.to_le_bytes())
        .chain(rgba.iter().copied())
    {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

fn artifact_ref(
    provider_id: &str,
    artifact_key: &str,
    frame: &FrameIdentity,
    pixel_hash: &str,
) -> String {
    format!(
        "slipway-debug-renderer://{}/{}/{}/{}/{}/{}",
        sanitize(provider_id),
        sanitize(&frame.surface_id),
        sanitize(&frame.surface_instance_id),
        frame.revision,
        frame.frame_index,
        sanitize(&format!("{artifact_key}-{pixel_hash}"))
    )
}

pub fn write_debug_rgba8_png_artifact(
    provider_id: &str,
    artifact_key: &str,
    frame: &FrameIdentity,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<DebugPngArtifactReceipt, DebugPngArtifactError> {
    if width == 0 || height == 0 {
        return Err(DebugPngArtifactError::ZeroSize);
    }
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(DebugPngArtifactError::ByteLengthOverflow)?;
    if rgba.len() != expected {
        return Err(DebugPngArtifactError::ByteLengthMismatch {
            expected,
            actual: rgba.len(),
        });
    }

    let pixel_hash = pixel_hash(width, height, rgba);
    let artifact_ref = artifact_ref(provider_id, artifact_key, frame, &pixel_hash);
    let dir = std::env::temp_dir().join("slipway-debug-renderer");
    fs::create_dir_all(&dir).map_err(|error| {
        DebugPngArtifactError::CreateDirectory(format!(
            "failed to create {}: {error}",
            dir.display()
        ))
    })?;
    let path = dir.join(format!("{}.png", sanitize(&artifact_ref)));
    let file = File::create(&path).map_err(|error| {
        DebugPngArtifactError::CreateFile(format!("failed to create {}: {error}", path.display()))
    })?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| {
        DebugPngArtifactError::Encode(format!("failed to write PNG header: {error}"))
    })?;
    writer.write_image_data(rgba).map_err(|error| {
        DebugPngArtifactError::Encode(format!("failed to write PNG data: {error}"))
    })?;
    Ok(DebugPngArtifactReceipt {
        artifact_ref,
        artifact_path: path.display().to_string(),
        pixel_hash,
    })
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn compare_artifact_bytes(
    left: &DebugRenderArtifact,
    right: &DebugRenderArtifact,
) -> ArtifactComparison {
    let left_total = left.width as u64 * left.height as u64;
    let right_total = right.width as u64 * right.height as u64;
    let total_pixel_count = left_total.max(right_total);
    let mut diagnostics = Vec::new();

    let mismatch_pixel_count = if left.width != right.width || left.height != right.height {
        diagnostics.push(diagnostic(
            Some(left.target.clone()),
            DiagnosticSeverity::Warning,
            "artifact-dimensions-differ",
            "artifact dimensions differ; comparison reports a full-frame mismatch",
        ));
        total_pixel_count
    } else {
        left.rgba
            .chunks_exact(4)
            .zip(right.rgba.chunks_exact(4))
            .filter(|(left, right)| left != right)
            .count() as u64
    };

    let exact_match =
        mismatch_pixel_count == 0 && left.width == right.width && left.height == right.height;
    let normalized_difference = if total_pixel_count == 0 {
        0.0
    } else {
        mismatch_pixel_count as f32 / total_pixel_count as f32
    };

    ArtifactComparison {
        left_artifact_ref: left.artifact_ref.clone(),
        right_artifact_ref: right.artifact_ref.clone(),
        left_dimensions: (left.width, left.height),
        right_dimensions: (right.width, right.height),
        mismatch_pixel_count,
        total_pixel_count,
        exact_match,
        normalized_difference,
        diagnostics,
    }
}

fn approximation_diagnostic(
    shape: &ShapeDeclaration,
    code: impl Into<String>,
    message: impl Into<String>,
) -> Diagnostic {
    diagnostic(
        shape.id.as_ref().map(|id| WidgetId::from(id.clone())),
        DiagnosticSeverity::Info,
        code,
        message,
    )
}

fn diagnostic(
    target: Option<WidgetId>,
    severity: DiagnosticSeverity,
    code: impl Into<String>,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        target,
        severity,
        code: code.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slipway_core::{
        BaselineShift, BoxSpacing, ChildLayoutPlan, ChildLayoutResult, ChildLayoutSeed,
        ContentLocalRect, EdgeInsets, FontStyle, FontWeight, GeometryCaptureIntent,
        LayoutConstraints, LayoutInput, LayoutOutput, PaintOrderDeclaration, Point,
        RenderSurfaceDeclaration, ShapeDeclaration, Size, TargetLocalRect, TextDecoration,
        TextStyle, ViewDefinition, WidgetSlotAddress, prepare_leaf_layout, prepare_resolved_layout,
        validate_and_index_view_with_capture,
    };

    fn frame(width: f32, height: f32, index: u64) -> FrameIdentity {
        FrameIdentity {
            surface_id: "surface".to_string(),
            surface_instance_id: "instance".to_string(),
            revision: 1,
            frame_index: index,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size { width, height },
            },
        }
    }

    fn packet(frame: FrameIdentity, paint: Vec<PaintOp>) -> RenderPacket {
        let bounds = TargetLocalRect::new(frame.viewport);
        let input = LayoutInput {
            viewport: bounds,
            content: bounds,
            constraints: LayoutConstraints {
                min: frame.viewport.size,
                max: frame.viewport.size,
            },
        };
        let (frame, _, output) =
            slipway_core::ViewDefinitionInput::new(frame, input).into_layout_parts();
        let layout = prepare_leaf_layout(output, bounds);
        let mut packet = RenderPacket {
            target: WidgetId::from("root"),
            frame,
            layout,
            paint,
            surfaces: Vec::new(),
            diagnostics: Vec::new(),
            prepared_geometry: None,
        };
        prepare_packet(&mut packet);
        packet
    }

    fn prepare_packet(packet: &mut RenderPacket) {
        let view = ViewDefinition {
            target: packet.target.clone(),
            frame: packet.frame.clone(),
            layout: packet.layout.clone(),
            paint: packet.paint.clone(),
            paint_order: PaintOrderDeclaration::source_order(packet.target.clone()),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        packet.prepared_geometry = validate_and_index_view_with_capture(
            &view,
            GeometryCaptureIntent::RenderPacketEvidence,
        )
        .ok();
    }

    fn rect(id: &str, x: f32, y: f32, width: f32, height: f32) -> ShapeDeclaration {
        ShapeDeclaration {
            id: Some(id.to_string()),
            kind: ShapeKind::Rectangle,
            bounds: Rect {
                origin: Point { x, y },
                size: Size { width, height },
            },
            path: None,
            clip: None,
        }
    }

    fn clip(id: &str, x: f32, y: f32, width: f32, height: f32) -> ClipDeclaration {
        ClipDeclaration {
            id: Some(id.to_string()),
            bounds: Rect {
                origin: Point { x, y },
                size: Size { width, height },
            },
            path: None,
        }
    }

    fn color(red: f32, green: f32, blue: f32, alpha: f32) -> Color {
        Color {
            red,
            green,
            blue,
            alpha,
        }
    }

    fn non_clear_pixels(artifact: &DebugRenderArtifact) -> usize {
        artifact
            .rgba
            .chunks_exact(4)
            .filter(|pixel| *pixel != [0, 0, 0, 0])
            .count()
    }

    fn text_op(style: TextStyle) -> PaintOp {
        PaintOp::Text {
            bounds: Rect {
                origin: Point { x: 3.0, y: 12.0 },
                size: Size {
                    width: 84.0,
                    height: 34.0,
                },
            },
            content: "Styled debug text".to_string(),
            color: color(0.0, 0.0, 0.0, 1.0),
            style,
        }
    }

    fn text_pixel_hash(style: TextStyle) -> String {
        let mut renderer = CpuDebugRenderer::default();
        renderer
            .render_offscreen(packet(frame(96.0, 56.0, 20), vec![text_op(style)]))
            .expect("text render succeeds")
            .pixel_hash
            .expect("pixel hash")
    }

    #[test]
    fn renderer_evidence_has_inspectable_png_artifact_and_pixel_hash() {
        let mut renderer = CpuDebugRenderer::default();
        let evidence = renderer
            .render_offscreen(packet(
                frame(32.0, 24.0, 1),
                vec![PaintOp::Fill {
                    shape: rect("fill", 1.0, 1.0, 8.0, 8.0),
                    color: color(1.0, 0.0, 0.0, 1.0),
                }],
            ))
            .expect("render succeeds");

        assert_eq!(evidence.provider_id, DEFAULT_PROVIDER_ID);
        assert_eq!(
            evidence.source,
            EvidenceSource::canonical_offscreen(DEFAULT_PROVIDER_ID)
        );
        assert!(evidence.artifact_ref.is_some());
        let artifact_path = evidence
            .artifact_path
            .as_deref()
            .expect("PNG artifact path");
        assert!(std::path::Path::new(artifact_path).exists());
        assert!(evidence.pixel_hash.is_some());
        assert_eq!(evidence.width, Some(32));
        assert_eq!(evidence.height, Some(24));
        assert_eq!(renderer.artifacts().len(), 1);
        assert_eq!(
            renderer.artifacts()[0].artifact_path.as_deref(),
            Some(artifact_path)
        );
    }

    #[test]
    fn artifact_dimensions_match_viewport() {
        let mut renderer = CpuDebugRenderer::default();
        let evidence = renderer
            .render_offscreen(packet(frame(19.2, 10.1, 2), Vec::new()))
            .expect("render succeeds");
        let artifact = renderer
            .artifact(evidence.artifact_ref.as_deref().expect("artifact ref"))
            .expect("artifact stored");

        assert_eq!(artifact.width, 20);
        assert_eq!(artifact.height, 11);
        assert_eq!(artifact.rgba.len(), 20 * 11 * 4);
    }

    #[test]
    fn fill_stroke_text_and_group_change_pixels_from_clear_color() {
        let mut renderer = CpuDebugRenderer::default();
        let paint = vec![PaintOp::Group {
            id: Some("group".to_string()),
            clip: None,
            ops: vec![
                PaintOp::Fill {
                    shape: rect("fill", 2.0, 2.0, 10.0, 8.0),
                    color: color(1.0, 0.0, 0.0, 0.8),
                },
                PaintOp::Stroke {
                    shape: ShapeDeclaration {
                        id: Some("circle".to_string()),
                        kind: ShapeKind::Circle,
                        bounds: Rect {
                            origin: Point { x: 14.0, y: 4.0 },
                            size: Size {
                                width: 12.0,
                                height: 12.0,
                            },
                        },
                        path: None,
                        clip: None,
                    },
                    color: color(0.0, 0.6, 1.0, 1.0),
                    width: 2.0,
                },
                PaintOp::Text {
                    bounds: Rect {
                        origin: Point { x: 4.0, y: 18.0 },
                        size: Size {
                            width: 36.0,
                            height: 12.0,
                        },
                    },
                    content: "debug".to_string(),
                    color: color(0.1, 0.1, 0.1, 1.0),
                    style: TextStyle::plain(),
                },
            ],
        }];
        let evidence = renderer
            .render_offscreen(packet(frame(48.0, 36.0, 3), paint))
            .expect("render succeeds");
        let artifact = renderer
            .artifact(evidence.artifact_ref.as_deref().expect("artifact ref"))
            .expect("artifact stored");

        assert!(non_clear_pixels(artifact) > 0);
        assert!(
            artifact
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "text-placeholder-rasterized")
        );
    }

    #[test]
    fn group_layer_and_shape_clips_limit_debug_pixels() {
        let mut renderer = CpuDebugRenderer::default();
        let evidence = renderer
            .render_offscreen(packet(
                frame(20.0, 20.0, 23),
                vec![
                    PaintOp::Group {
                        id: Some("group-clip".to_string()),
                        clip: Some(clip("group-clip", 0.0, 0.0, 5.0, 20.0)),
                        ops: vec![PaintOp::Fill {
                            shape: rect("group-fill", 0.0, 0.0, 20.0, 20.0),
                            color: color(1.0, 0.0, 0.0, 1.0),
                        }],
                    },
                    PaintOp::Layer {
                        id: Some("layer-clip".to_string()),
                        key: slipway_core::PaintLayerKey::ordered(1, 0),
                        input_transparency: slipway_core::PaintInputTransparency::Opaque,
                        wheel_transparency: None,
                        clip: Some(clip("layer-clip", 10.0, 0.0, 5.0, 20.0)),
                        ops: vec![PaintOp::Fill {
                            shape: rect("layer-fill", 0.0, 0.0, 20.0, 20.0),
                            color: color(0.0, 1.0, 0.0, 1.0),
                        }],
                    },
                    PaintOp::Fill {
                        shape: ShapeDeclaration {
                            clip: Some(clip("shape-clip", 15.0, 0.0, 5.0, 20.0)),
                            ..rect("shape-fill", 0.0, 0.0, 20.0, 20.0)
                        },
                        color: color(0.0, 0.0, 1.0, 1.0),
                    },
                ],
            ))
            .expect("render succeeds");
        let artifact = renderer
            .artifact(evidence.artifact_ref.as_deref().expect("artifact ref"))
            .expect("artifact stored");

        assert_eq!(non_clear_pixels(artifact), 300);
    }

    #[test]
    fn text_style_changes_deterministic_pixel_hash() {
        let base = TextStyle::plain();
        let base_hash = text_pixel_hash(base.clone());
        let variants = [
            TextStyle {
                font_size: 22.0,
                ..base.clone()
            },
            TextStyle {
                font_weight: FontWeight::Bold,
                ..base.clone()
            },
            TextStyle {
                font_style: FontStyle::Italic,
                ..base.clone()
            },
            TextStyle {
                decoration: TextDecoration {
                    underline: true,
                    strikethrough: false,
                },
                ..base.clone()
            },
            TextStyle {
                decoration: TextDecoration {
                    underline: false,
                    strikethrough: true,
                },
                ..base.clone()
            },
            TextStyle {
                baseline: BaselineShift::Superscript,
                ..base.clone()
            },
            TextStyle {
                baseline: BaselineShift::Subscript,
                ..base
            },
        ];

        for variant in variants {
            assert_ne!(text_pixel_hash(variant), base_hash);
        }
    }

    #[test]
    fn path_shape_renders_as_canonical_offscreen_data_with_diagnostic() {
        let mut renderer = CpuDebugRenderer::default();
        let path_shape = ShapeDeclaration {
            id: Some("path".to_string()),
            kind: ShapeKind::Path,
            bounds: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 24.0,
                    height: 18.0,
                },
            },
            path: Some(PathDeclaration {
                commands: vec![
                    PathCommand::MoveTo(Point { x: 2.0, y: 16.0 }),
                    PathCommand::QuadraticTo {
                        control: Point { x: 12.0, y: 2.0 },
                        to: Point { x: 22.0, y: 16.0 },
                    },
                ],
            }),
            clip: None,
        };

        let evidence = renderer
            .render_offscreen(packet(
                frame(28.0, 20.0, 22),
                vec![PaintOp::Fill {
                    shape: path_shape,
                    color: color(0.2, 0.8, 0.1, 1.0),
                }],
            ))
            .expect("path render succeeds");
        let artifact = renderer
            .artifact(evidence.artifact_ref.as_deref().expect("artifact ref"))
            .expect("artifact stored");

        assert_eq!(
            evidence.source,
            EvidenceSource::canonical_offscreen(DEFAULT_PROVIDER_ID)
        );
        assert!(non_clear_pixels(artifact) > 0);
        assert!(
            evidence
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "path-fill-approximated")
        );
        assert!(
            evidence
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "path-curve-stroke-approximated")
        );
    }

    #[test]
    fn text_renderer_uses_real_glyph_path_for_korean_text() {
        let mut renderer = CpuDebugRenderer::default();
        let evidence = renderer
            .render_offscreen(packet(
                frame(96.0, 56.0, 21),
                vec![PaintOp::Text {
                    bounds: Rect {
                        origin: Point { x: 3.0, y: 12.0 },
                        size: Size {
                            width: 90.0,
                            height: 34.0,
                        },
                    },
                    content: "실시간 도시".to_string(),
                    color: color(0.0, 0.0, 0.0, 1.0),
                    style: TextStyle {
                        font_size: 18.0,
                        font_weight: FontWeight::Weight(650),
                        font_style: FontStyle::Italic,
                        decoration: TextDecoration {
                            underline: true,
                            strikethrough: true,
                        },
                        baseline: BaselineShift::Superscript,
                        ..TextStyle::plain()
                    },
                }],
            ))
            .expect("text render succeeds");

        assert!(
            evidence
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "text-placeholder-rasterized")
        );
        assert!(evidence.pixel_hash.is_some());
    }

    fn ink_bbox(artifact: &DebugRenderArtifact) -> (i32, i32, i32, i32) {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..artifact.height as i32 {
            for x in 0..artifact.width as i32 {
                let offset = ((y as u32 * artifact.width + x as u32) * 4) as usize;
                if artifact.rgba[offset..offset + 4] != [0, 0, 0, 0] {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        assert!(x0 <= x1, "expected ink pixels in the artifact");
        (x0, y0, x1, y1)
    }

    fn ink_row_bands(artifact: &DebugRenderArtifact) -> usize {
        let mut bands = 0;
        let mut in_band = false;
        for y in 0..artifact.height as i32 {
            let row_has_ink = (0..artifact.width as i32).any(|x| {
                let offset = ((y as u32 * artifact.width + x as u32) * 4) as usize;
                artifact.rgba[offset..offset + 4] != [0, 0, 0, 0]
            });
            if row_has_ink && !in_band {
                bands += 1;
            }
            in_band = row_has_ink;
        }
        bands
    }

    // Declared-alignment honor pin (roadmap Phase 6 item 3b, closing the
    // Step 216 nonclaim: the CPU renderer used to draw every text op
    // top-left regardless of the declaration, so offscreen evidence could
    // not show centering). The anchoring rule must match the visible
    // backends': the wrapped block anchors vertically, each line anchors
    // horizontally, within the op rect. Reverting `draw_text_glyphs` to
    // the top-left caret fails every non-default assertion here.
    #[test]
    fn declared_alignment_anchors_ink_within_op_rect() {
        let op_rect = |style: TextStyle| PaintOp::Text {
            bounds: Rect {
                origin: Point { x: 10.0, y: 10.0 },
                size: Size {
                    width: 100.0,
                    height: 40.0,
                },
            },
            content: "mid".to_string(),
            color: color(0.0, 0.0, 0.0, 1.0),
            style,
        };
        let render = |style: TextStyle, index: u64| {
            let mut renderer = CpuDebugRenderer::default();
            let evidence = renderer
                .render_offscreen(packet(frame(120.0, 60.0, index), vec![op_rect(style)]))
                .expect("text render succeeds");
            let artifact = renderer
                .artifact(evidence.artifact_ref.as_deref().expect("artifact ref"))
                .expect("artifact stored");
            ink_bbox(artifact)
        };

        // Default equivalence: unspecified alignment hugs the top-left.
        let (dx0, dy0, dx1, dy1) = render(TextStyle::plain(), 30);
        assert!(dx0 - 10 <= 3, "default ink starts at the left edge: {dx0}");
        assert!(dy0 - 10 <= 8, "default ink starts at the top: {dy0}");

        // Centered: the ink centroid sits on the rect center both ways.
        let (cx0, cy0, cx1, cy1) = render(TextStyle::plain().centered(), 31);
        let ink_center_x = (cx0 + cx1) / 2;
        let ink_center_y = (cy0 + cy1) / 2;
        assert!(
            (ink_center_x - 60).abs() <= 2,
            "centered ink x-center {ink_center_x} must sit on the rect center 60"
        );
        assert!(
            (ink_center_y - 30).abs() <= 4,
            "centered ink y-center {ink_center_y} must sit near the rect center 30"
        );
        assert!(cx0 > dx0, "centered ink moved right of the default");
        assert!(cy0 > dy0, "centered ink moved below the default");

        // End/Bottom: the ink hugs the right edge and sits below both
        // the default and the centered renders.
        let (ex0, _ey0, ex1, ey1) = render(
            TextStyle::plain()
                .with_align_x(slipway_core::TextAlignX::End)
                .with_align_y(slipway_core::TextAlignY::Bottom),
            32,
        );
        assert!(
            110 - ex1 <= 3,
            "end-aligned ink must reach the right edge 110: {ex1}"
        );
        assert!(ex0 > cx0, "end ink sits right of centered ink");
        assert!(ey1 > cy1, "bottom ink sits below centered ink");
        assert!(ey1 > dy1, "bottom ink sits below default ink");
        assert!(dx1 < 110, "default ink does not reach the right edge");
    }

    // Declared wrap opt-out honor pin (NC-4): the default word wrap
    // breaks a too-wide CJK label into multiple ink rows at the rect
    // width; `TextWrap::None` keeps ONE row and clips at the rect edge —
    // matching both visible backends' contract. Reverting the renderer
    // (or either backend mapping) to unconditional word wrap fails the
    // single-row assertion.
    #[test]
    fn declared_wrap_optout_renders_single_row_where_word_wraps() {
        let text_in_narrow_rect = |style: TextStyle| PaintOp::Text {
            bounds: Rect {
                origin: Point { x: 4.0, y: 4.0 },
                size: Size {
                    width: 72.0,
                    height: 72.0,
                },
            },
            content: "줄바꿈 없는 한국어 라벨".to_string(),
            color: color(0.0, 0.0, 0.0, 1.0),
            style,
        };
        let render = |style: TextStyle, index: u64| {
            let mut renderer = CpuDebugRenderer::default();
            let evidence = renderer
                .render_offscreen(packet(
                    frame(96.0, 80.0, index),
                    vec![text_in_narrow_rect(style)],
                ))
                .expect("text render succeeds");
            let artifact = renderer
                .artifact(evidence.artifact_ref.as_deref().expect("artifact ref"))
                .expect("artifact stored");
            (ink_row_bands(artifact), ink_bbox(artifact))
        };

        let (word_bands, _) = render(TextStyle::plain(), 33);
        assert!(
            word_bands >= 2,
            "default word wrap must break the label into multiple rows, got {word_bands}"
        );

        let (none_bands, (_, _, nx1, _)) = render(TextStyle::plain().no_wrap(), 34);
        assert_eq!(
            none_bands, 1,
            "TextWrap::None must keep the label on a single row"
        );
        // The single line is wider than the rect and clips AT the rect
        // edge (x = 4 + 72 = 76, exclusive bbox max 75).
        assert!(
            (75 - nx1).abs() <= 1,
            "no-wrap ink must clip at the rect edge 75: {nx1}"
        );
    }

    #[test]
    fn comparison_reports_exact_match_and_mismatch() {
        let mut renderer = CpuDebugRenderer::default();
        let first = renderer
            .render_offscreen(packet(
                frame(24.0, 24.0, 4),
                vec![PaintOp::Fill {
                    shape: rect("fill", 0.0, 0.0, 8.0, 8.0),
                    color: color(1.0, 0.0, 0.0, 1.0),
                }],
            ))
            .expect("first render");
        let second = renderer
            .render_offscreen(packet(
                frame(24.0, 24.0, 5),
                vec![PaintOp::Fill {
                    shape: rect("fill", 0.0, 0.0, 8.0, 8.0),
                    color: color(1.0, 0.0, 0.0, 1.0),
                }],
            ))
            .expect("second render");
        let changed = renderer
            .render_offscreen(packet(
                frame(24.0, 24.0, 6),
                vec![PaintOp::Fill {
                    shape: rect("fill", 0.0, 0.0, 12.0, 8.0),
                    color: color(0.0, 0.0, 1.0, 1.0),
                }],
            ))
            .expect("changed render");

        let exact = renderer
            .compare_artifacts(
                first.artifact_ref.as_deref().expect("first ref"),
                second.artifact_ref.as_deref().expect("second ref"),
            )
            .expect("compare exact");
        assert!(exact.exact_match);
        assert_eq!(exact.mismatch_pixel_count, 0);

        let mismatch = renderer
            .compare_artifacts(
                first.artifact_ref.as_deref().expect("first ref"),
                changed.artifact_ref.as_deref().expect("changed ref"),
            )
            .expect("compare mismatch");
        assert!(!mismatch.exact_match);
        assert!(mismatch.mismatch_pixel_count > 0);
        assert!(mismatch.normalized_difference > 0.0);
    }

    #[test]
    fn surface_declarations_return_evidence_with_diagnostic() {
        let mut renderer = CpuDebugRenderer::default();
        let mut packet = packet(frame(16.0, 16.0, 7), Vec::new());
        packet.surfaces.push(RenderSurfaceDeclaration {
            id: WidgetId::from("surface"),
            provider_id: "external".to_string(),
            bounds: packet.frame.viewport,
            payload_ref: Some("payload".to_string()),
            dirty_regions: Vec::new(),
            capabilities: Vec::new(),
        });

        let evidence = renderer.render_offscreen(packet).expect("render succeeds");

        assert!(
            evidence
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "render-surfaces-not-rasterized")
        );
    }

    #[test]
    fn public_rgba8_sink_preserves_hash_reference_path_and_png_metadata() {
        let receipt = write_debug_rgba8_png_artifact(
            "presented/provider",
            "root instance",
            &frame(1.0, 1.0, 77),
            1,
            1,
            &[1, 2, 3, 4],
        )
        .expect("valid tight RGBA writes");

        assert_eq!(receipt.pixel_hash, "fnv1a64:ef73dc3a80c8da6d");
        assert!(
            receipt
                .artifact_ref
                .starts_with("slipway-debug-renderer://presented_provider/surface/instance/1/77/")
        );
        assert!(
            receipt
                .artifact_ref
                .ends_with("root_instance-fnv1a64_ef73dc3a80c8da6d")
        );
        assert!(receipt.artifact_path.ends_with(".png"));

        let file = File::open(&receipt.artifact_path).expect("PNG artifact opens");
        let decoder = png::Decoder::new(std::io::BufReader::new(file));
        let reader = decoder.read_info().expect("PNG header decodes");
        assert_eq!(reader.info().width, 1);
        assert_eq!(reader.info().height, 1);
        assert_eq!(reader.info().color_type, png::ColorType::Rgba);
        assert_eq!(reader.info().bit_depth, png::BitDepth::Eight);
        let _ = fs::remove_file(receipt.artifact_path);
    }

    #[test]
    fn public_rgba8_sink_rejects_zero_size_and_malformed_length_before_writing() {
        let frame = frame(1.0, 1.0, 78);
        assert_eq!(
            write_debug_rgba8_png_artifact("provider", "key", &frame, 0, 1, &[]),
            Err(DebugPngArtifactError::ZeroSize)
        );
        assert_eq!(
            write_debug_rgba8_png_artifact("provider", "key", &frame, 2, 1, &[0; 4]),
            Err(DebugPngArtifactError::ByteLengthMismatch {
                expected: 8,
                actual: 4,
            })
        );
    }

    fn layout_with_placement(
        frame: &FrameIdentity,
        spacing: BoxSpacing,
        border: Rect,
        addressed: bool,
    ) -> LayoutOutput {
        let child = WidgetId::from("panel");
        let slot = addressed.then(|| WidgetSlotAddress::new(child.clone(), 0));
        let seed = ChildLayoutSeed {
            child: child.clone(),
            local_state_slot: slot,
        };
        let outer = Rect {
            origin: Point {
                x: border.origin.x - spacing.margin.left,
                y: border.origin.y - spacing.margin.top,
            },
            size: Size {
                width: border.size.width + spacing.margin.horizontal(),
                height: border.size.height + spacing.margin.vertical(),
            },
        };
        let plan = ChildLayoutPlan::requested_outer(
            seed.clone(),
            ContentLocalRect::new(outer),
            LayoutConstraints {
                min: outer.size,
                max: outer.size,
            },
            spacing,
        );
        let child_bounds = TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: border.size,
        });
        let child_input = LayoutInput {
            viewport: child_bounds,
            content: child_bounds,
            constraints: LayoutConstraints {
                min: border.size,
                max: border.size,
            },
        };
        let (_, _, child_output) =
            slipway_core::ViewDefinitionInput::new(frame.clone(), child_input).into_layout_parts();
        let result = ChildLayoutResult {
            seed,
            layout: prepare_leaf_layout(child_output, child_bounds),
            diagnostics: Vec::new(),
        };
        let root_bounds = TargetLocalRect::new(frame.viewport);
        let root_input = LayoutInput {
            viewport: root_bounds,
            content: root_bounds,
            constraints: LayoutConstraints {
                min: frame.viewport.size,
                max: frame.viewport.size,
            },
        };
        let (_, _, root_output) =
            slipway_core::ViewDefinitionInput::new(frame.clone(), root_input).into_layout_parts();
        prepare_resolved_layout(root_output, root_bounds, [(plan, result)]).unwrap()
    }

    #[test]
    fn artifact_records_asymmetric_box_geometry_without_margin_pixels() {
        let spacing = BoxSpacing::new(
            EdgeInsets::trbl(8.0, 24.0, 12.0, 4.0),
            EdgeInsets::trbl(6.0, 28.0, 18.0, 10.0),
        );
        let mut packet = packet(frame(160.0, 100.0, 80), Vec::new());
        packet.layout = layout_with_placement(
            &packet.frame,
            spacing,
            Rect {
                origin: Point { x: 20.0, y: 15.0 },
                size: Size {
                    width: 100.0,
                    height: 60.0,
                },
            },
            true,
        );
        prepare_packet(&mut packet);
        let mut renderer = CpuDebugRenderer::default();
        let rendered = renderer.render_offscreen(packet).expect("valid geometry");
        let artifact = renderer
            .artifact(rendered.artifact_ref.as_deref().unwrap())
            .unwrap();
        let geometry = &artifact.box_geometry[0];

        assert_eq!(
            geometry.target_local.border.into_rect(),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 60.0
                },
            }
        );
        assert_eq!(
            geometry.target_local.content.into_rect(),
            Rect {
                origin: Point { x: 10.0, y: 6.0 },
                size: Size {
                    width: 62.0,
                    height: 36.0
                },
            }
        );
        assert_eq!(
            geometry.parent_local.outer.into_rect(),
            Rect {
                origin: Point { x: 16.0, y: 7.0 },
                size: Size {
                    width: 128.0,
                    height: 80.0
                },
            }
        );
        assert_eq!(
            geometry.final_presented.content,
            Rect {
                origin: Point { x: 30.0, y: 21.0 },
                size: Size {
                    width: 62.0,
                    height: 36.0
                },
            }
        );
        assert_eq!(
            geometry.effective_clip_final,
            geometry.final_presented.border
        );
        assert_eq!(
            non_clear_pixels(artifact),
            0,
            "margin is evidence, never paint"
        );
    }

    #[test]
    fn oversized_padding_is_valid_and_records_clamped_empty_content() {
        let spacing = BoxSpacing::ZERO.with_padding(EdgeInsets::trbl(70.0, 80.0, 90.0, 120.0));
        let mut packet = packet(frame(160.0, 100.0, 81), Vec::new());
        packet.layout = layout_with_placement(
            &packet.frame,
            spacing,
            Rect {
                origin: Point { x: 20.0, y: 15.0 },
                size: Size {
                    width: 100.0,
                    height: 60.0,
                },
            },
            true,
        );
        prepare_packet(&mut packet);
        let mut renderer = CpuDebugRenderer::default();
        let evidence = renderer
            .render_offscreen(packet)
            .expect("oversized padding is valid");
        let geometry = &renderer
            .artifact(evidence.artifact_ref.as_deref().unwrap())
            .unwrap()
            .box_geometry[0];
        assert_eq!(
            geometry.target_local.content.origin,
            Point { x: 100.0, y: 60.0 }
        );
        assert_eq!(
            geometry.target_local.content.size,
            Size {
                width: 0.0,
                height: 0.0
            }
        );
    }

    #[test]
    fn missing_address_refuses_before_artifact_creation() {
        let mut renderer = CpuDebugRenderer::default();
        let mut fallback = packet(frame(160.0, 100.0, 83), Vec::new());
        fallback.layout = layout_with_placement(
            &fallback.frame,
            BoxSpacing::ZERO,
            Rect {
                origin: Point { x: 20.0, y: 15.0 },
                size: Size {
                    width: 100.0,
                    height: 60.0,
                },
            },
            false,
        );
        prepare_packet(&mut fallback);
        let refusal = renderer
            .render_offscreen(fallback)
            .expect_err("ID fallback is forbidden");
        assert_eq!(
            refusal.diagnostics.last().unwrap().code,
            PREPARED_GEOMETRY_REQUIRED
        );
        assert!(renderer.artifacts().is_empty());
    }

    #[test]
    fn zero_spacing_preserves_pixels_and_hash() {
        let paint = vec![PaintOp::Fill {
            shape: rect("same", 2.0, 3.0, 9.0, 7.0),
            color: color(0.2, 0.7, 0.4, 1.0),
        }];
        let mut renderer = CpuDebugRenderer::default();
        let baseline = renderer
            .render_offscreen(packet(frame(24.0, 24.0, 84), paint.clone()))
            .unwrap();
        let mut with_geometry = packet(frame(24.0, 24.0, 85), paint);
        with_geometry.layout = layout_with_placement(
            &with_geometry.frame,
            BoxSpacing::ZERO,
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 24.0,
                    height: 24.0,
                },
            },
            true,
        );
        prepare_packet(&mut with_geometry);
        let spaced = renderer.render_offscreen(with_geometry).unwrap();
        assert_eq!(baseline.pixel_hash, spaced.pixel_hash);
    }
}
