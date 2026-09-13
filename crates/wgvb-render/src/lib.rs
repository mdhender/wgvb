//! Bounded viewport rendering for WGVB worlds.
//!
//! Generation is effectively unbounded; rendering is not. Every render request
//! defines a finite viewport and an explicit pixel scale. Renderer pixel
//! coordinates must never feed back into terrain generation.
//! See `DESIGN.md` sections 7.1 and 29.
//!
//! # Why `hexx` lives here and not in the core
//!
//! `hexx` is built on `glam`, so its layout math is `f32`. That is right for
//! pixel geometry and wrong for the `f64` canonical world space the generator
//! samples. Keeping `hexx` out of `wgvb` makes the mistake structurally
//! impossible: the core crate cannot see the types that would let generation
//! coordinates round-trip through `f32`.
//!
//! Two rules follow, and both are load-bearing:
//!
//! - **`hexx` only ever sees viewport-relative offsets.** A [`Viewport`]
//!   converts to absolute coordinates in `i64` and `Coord` space, so the numbers
//!   handed to `glam` stay bounded by the viewport size rather than reaching the
//!   world's `±32,767` extent, where `f32` has already lost integer precision.
//! - **No `hexx` value ever becomes a [`Coord`].** Pixels come from coordinates;
//!   coordinates never come from pixels except through [`Viewport::locate`],
//!   which reconstructs the coordinate by integer arithmetic from the offset
//!   cell rather than by rounding a world position.
//!
//! # Orientation
//!
//! Every player sees a flat-top layout. `hexx` and WGVB number their directions
//! in opposite cyclic senses — `hexx_index = (-d).rem_euclid(6)` — and `hexx`
//! labels its `(0, -1)` edge `FLAT_NORTH`, which is WGVB direction **2**, not
//! 0. So on an unrotated viewport, absolute direction 2 points to the top of the
//! image. [`tests::direction_two_points_to_the_top_of_the_image`] pins that
//! whole composition rather than leaving it to be rediscovered.
//!
//! Per-player rotation — each player is assigned an origin hex *and* a rotation,
//! so one player's north is absolute direction `k` and another's is not — is a
//! render-layer concern that belongs here, but it arrives with the player state
//! that stores it. This phase renders the unrotated absolute frame.

mod palette;

use hexx::{Hex, HexLayout, HexOrientation, OffsetHexMode, Vec2};
use wgvb::{Component, Coord, Generator, Sample};

pub use palette::{BACKGROUND, color};

/// Palette and symbol-rule version. Cached or golden-compared rendered output
/// is invalid across a change to this value.
pub const RENDER_VERSION: u32 = 1;

/// Largest image this crate will produce, in pixels.
///
/// Rendering is bounded by design; an accidental `--cols 100000` should be a
/// typed error rather than an allocation failure.
pub const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;

/// Largest image edge, in pixels. Some decoders reject wider images, and a
/// single row that long is a mistake rather than a request.
pub const MAX_IMAGE_EDGE: u32 = 32_768;

/// Why a render request was rejected. One variant per gate, so tests assert on
/// variants rather than on message strings. See `DESIGN.md` section 19.1.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("a viewport must cover at least one tile, got {cols} x {rows}")]
    EmptyViewport { cols: u32, rows: u32 },
    #[error("hex radius must be finite and at least 1 pixel, got {0}")]
    HexRadius(f32),
    #[error("image would be {width} x {height} pixels, over the {limit} pixel limit")]
    TooLarge { width: u64, height: u64, limit: u64 },
    #[error(
        "a centered viewport needs odd tile counts so that a center cell exists, got {cols} x {rows}"
    )]
    EvenViewport { cols: u32, rows: u32 },
    #[error("unknown layer {0:?}")]
    UnknownLayer(String),
    #[error("encoding the image failed")]
    Encode(#[from] png::EncodingError),
}

/// Converts an axial coordinate pair to `hexx`'s `Hex`.
///
/// **The one adapter.** `hexx` names the axes `x` and `y`; WGVB names them `q`
/// and `r`. Settling that rename in a single function is the point — a second
/// conversion written inline somewhere is how the two names drift apart.
///
/// Pass *viewport-relative* components. `Hex` is `i32`-backed so the conversion
/// is lossless for any [`Component`], but everything `hexx` does with the result
/// runs through `f32` layout math, which cannot represent every `i32` and
/// certainly not the world's `±32,767` extent scaled by a pixel radius.
#[must_use]
pub fn to_hex(q: Component, r: Component) -> Hex {
    Hex::new(i32::from(q), i32::from(r))
}

/// A diagnostic scalar layer.
///
/// Only the fields that exist at this phase appear here. Temperature, moisture,
/// climate, and terrain arrive with the phases that generate them; in
/// particular there is deliberately no terrain layer while
/// [`wgvb::Tile::terrain`] is provisional, because a terrain image nobody
/// should trust is worse than no terrain image.
///
/// Almost every layer reads one scalar out of a [`Sample`], which is what keeps
/// the renderer from needing a second traversal of the world per layer.
/// [`Layer::Relief`] is the exception: relief costs seven elevation
/// evaluations, so a sample does not carry it and the layer asks the generator
/// directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Continentalness,
    Regional,
    Local,
    Detail,
    ElevationRaw,
    /// The elevation scalar of `DESIGN.md` section 14 — the real one, with
    /// region uplift, ridge structure, and the contrast shaping folded in.
    ///
    /// This is the layer the phase 4 exit condition is read off. What to look
    /// for over several widely separated windows: oceans that hold together
    /// rather than dissolving into lakes, coastlines that wander instead of
    /// following the noise lattice, and lowland giving way to upland inland
    /// rather than at random.
    Elevation,
    /// Local relief, in `[0, 1]`: flat ground at the bottom of the palette,
    /// steep ground at the top.
    ///
    /// Only the upper half of the ramp is used, because relief is unsigned.
    Relief,
    /// The ridge structure term, before the region roughness that scales it.
    ///
    /// Crests read as bright lines. If they are not lines — if they are blobs,
    /// or if they run the same way over the whole map — the directional average
    /// or the blended ridge orientation is not doing its job.
    Ridge,
    /// The blended region roughness bias, which is what decides how strongly
    /// [`Layer::Ridge`] contributes at each tile. Rendered next to the ridge
    /// layer it explains where the mountain belts are and are not.
    Roughness,
    /// The blended regional elevation bias of `DESIGN.md` sections 11 and 12 —
    /// the "region influence" layer of section 29.
    ///
    /// This is the layer the phase 3 exit condition is read off. Two things to
    /// look for: distant areas that plainly differ from one another, and *no*
    /// lattice. Anchors sit every 128 and every 512 hexes, so a blend that had
    /// gone wrong would draw a grid of parallelograms at those spacings, which
    /// is unmistakable at a small hex radius over a wide window.
    RegionInfluence,
}

impl Layer {
    /// Every layer, in the order the command line lists them.
    pub const ALL: [Layer; 10] = [
        Layer::Continentalness,
        Layer::Regional,
        Layer::Local,
        Layer::Detail,
        Layer::ElevationRaw,
        Layer::Elevation,
        Layer::Relief,
        Layer::Ridge,
        Layer::Roughness,
        Layer::RegionInfluence,
    ];

    /// The layer's command-line name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Layer::Continentalness => "continentalness",
            Layer::Regional => "regional",
            Layer::Local => "local",
            Layer::Detail => "detail",
            Layer::ElevationRaw => "elevation-raw",
            Layer::Elevation => "elevation",
            Layer::Relief => "relief",
            Layer::Ridge => "ridge",
            Layer::Roughness => "roughness",
            Layer::RegionInfluence => "region-influence",
        }
    }

    /// Parses a layer name.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::UnknownLayer`] for a name no layer has. An unknown
    /// layer is rejected rather than silently defaulted, for the same reason
    /// configuration rejects unknown fields.
    pub fn parse(name: &str) -> Result<Layer, RenderError> {
        Layer::ALL
            .into_iter()
            .find(|layer| layer.name() == name)
            .ok_or_else(|| RenderError::UnknownLayer(name.to_string()))
    }

    /// This layer's scalar at one coordinate.
    ///
    /// One generator call for every layer but [`Layer::Relief`], which needs
    /// the six neighboring elevations and so asks for relief directly rather
    /// than making every other layer pay for it.
    #[must_use]
    pub fn value(self, generator: &Generator, coord: Coord) -> f64 {
        match self {
            Layer::Relief => generator.relief(coord),
            other => other.of_sample(&generator.sample(coord)),
        }
    }

    /// Reads this layer's scalar out of a sample.
    ///
    /// # Panics
    ///
    /// Panics for [`Layer::Relief`], which a [`Sample`] does not carry. Private
    /// for that reason; [`Layer::value`] is the total function.
    fn of_sample(self, sample: &Sample) -> f64 {
        match self {
            Layer::Continentalness => sample.continentalness,
            Layer::Regional => sample.regional,
            Layer::Local => sample.local,
            Layer::Detail => sample.detail,
            Layer::ElevationRaw => sample.elevation_raw,
            Layer::Elevation => sample.elevation,
            Layer::Ridge => sample.ridge,
            Layer::Roughness => sample.roughness,
            Layer::RegionInfluence => sample.regional_uplift,
            Layer::Relief => unreachable!("relief is not carried by a sample"),
        }
    }
}

/// A finite window into the unbounded world.
///
/// `cols` and `rows` are tile counts; `hex_radius` is a pixel dimension. The
/// window is a rectangle of even-`q` offset cells, which is what makes a
/// rectangular image full of hexes rather than a ragged parallelogram.
#[derive(Debug, Clone)]
pub struct Viewport {
    origin: Coord,
    cols: u32,
    rows: u32,
    hex_radius: f32,
    layout: HexLayout,
    width: u32,
    height: u32,
}

/// The offset scheme. Flat-top columns, even columns shoved down.
const OFFSET_MODE: OffsetHexMode = OffsetHexMode::Even;
/// The orientation every player sees.
const ORIENTATION: HexOrientation = HexOrientation::Flat;

impl Viewport {
    /// Defines a viewport and computes the image it will fill.
    ///
    /// # Errors
    ///
    /// Rejects an empty tile range, a hex radius under one pixel, and any
    /// request whose image would exceed [`MAX_IMAGE_PIXELS`] or
    /// [`MAX_IMAGE_EDGE`].
    pub fn new(
        origin: Coord,
        cols: u32,
        rows: u32,
        hex_radius: f32,
    ) -> Result<Viewport, RenderError> {
        if cols == 0 || rows == 0 {
            return Err(RenderError::EmptyViewport { cols, rows });
        }
        if !hex_radius.is_finite() || hex_radius < 1.0 {
            return Err(RenderError::HexRadius(hex_radius));
        }

        // Pixel extents come from the four corner cells of the offset
        // rectangle. `hexx` sees only the offsets, never an absolute
        // coordinate.
        let mut layout = HexLayout::flat().with_hex_size(hex_radius);
        let half = layout.rect_size() / 2.0;

        let mut min = Vec2::new(f32::MAX, f32::MAX);
        let mut max = Vec2::new(f32::MIN, f32::MIN);
        for col in [0_i32, i32_of(cols - 1)] {
            for row in [0_i32, i32_of(rows - 1)] {
                let center = layout.hex_to_world_pos(offset_hex(col, row));
                min = min.min(center - half);
                max = max.max(center + half);
            }
        }
        // An odd column is shoved half a hex down, so the last row of an
        // odd-column viewport reaches further than any corner cell does.
        if cols > 1 {
            max.y += half.y;
        }

        let width = ceil_to_u32(max.x - min.x);
        let height = ceil_to_u32(max.y - min.y);
        let pixels = u64::from(width) * u64::from(height);
        if width > MAX_IMAGE_EDGE || height > MAX_IMAGE_EDGE || pixels > MAX_IMAGE_PIXELS {
            return Err(RenderError::TooLarge {
                width: u64::from(width),
                height: u64::from(height),
                limit: MAX_IMAGE_PIXELS,
            });
        }

        // Shift the layout so the leftmost, topmost hex corner lands on pixel
        // zero. This is the only place a pixel origin is chosen.
        layout.origin = Vec2::new(-min.x, -min.y);

        Ok(Viewport {
            origin,
            cols,
            rows,
            hex_radius,
            layout,
            width,
            height,
        })
    }

    /// Defines a viewport around a center tile rather than around its first
    /// tile.
    ///
    /// [`Viewport::new`] takes the window's *first* tile, which is what the
    /// renderer needs and not what a viewer asks for: "show me this coordinate"
    /// names the middle of the window. The conversion is offset-scheme
    /// arithmetic, so it belongs to the crate that owns the offset scheme — a
    /// second copy of this living in a front end is how the layout convention
    /// drifts.
    ///
    /// **Both tile counts must be odd**, which is the whole reason this is a
    /// constructor and not a helper. An even count has no center cell, so the
    /// conversion would have to round, and a rounded conversion makes a scroll
    /// step followed by its opposite stop returning to where it started. Odd is
    /// enforced rather than documented.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::EmptyViewport`] for a zero tile count,
    /// [`RenderError::EvenViewport`] for an even one, and whatever
    /// [`Viewport::new`] rejects otherwise.
    pub fn centered_on(
        center: Coord,
        cols: u32,
        rows: u32,
        hex_radius: f32,
    ) -> Result<Viewport, RenderError> {
        if cols == 0 || rows == 0 {
            return Err(RenderError::EmptyViewport { cols, rows });
        }
        if cols.is_multiple_of(2) || rows.is_multiple_of(2) {
            return Err(RenderError::EvenViewport { cols, rows });
        }

        // The center cell of an odd window, and the offset from the first cell
        // to it. Subtracting in `i64` and letting `Coord` normalize is what
        // makes a window centered near a wrapped edge ordinary rather than a
        // special case.
        let hex = offset_hex(i32_of(cols / 2), i32_of(rows / 2));
        let origin = Coord::new(
            i64::from(center.q()) - i64::from(hex.x),
            i64::from(center.r()) - i64::from(hex.y),
        );
        Viewport::new(origin, cols, rows, hex_radius)
    }

    /// The tile at the viewport's `(0, 0)` offset cell.
    #[must_use]
    pub const fn origin(&self) -> Coord {
        self.origin
    }

    /// The tile in the exact center cell, when one exists.
    ///
    /// `None` for an even tile count in either axis, where there is no center
    /// cell to name. The inverse of [`Viewport::centered_on`].
    #[must_use]
    pub fn center(&self) -> Option<Coord> {
        (self.cols % 2 == 1 && self.rows % 2 == 1)
            .then(|| self.coord_at(self.cols / 2, self.rows / 2))
    }

    /// Tile counts across and down.
    #[must_use]
    pub const fn tile_counts(&self) -> (u32, u32) {
        (self.cols, self.rows)
    }

    /// Hex radius in pixels.
    #[must_use]
    pub const fn hex_radius(&self) -> f32 {
        self.hex_radius
    }

    /// Image dimensions in pixels.
    #[must_use]
    pub const fn image_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The absolute coordinate of one offset cell.
    ///
    /// The addition happens in `i64` and normalization happens in [`Coord`], so
    /// a viewport that runs off the edge of the canonical map wraps rather than
    /// overflowing — which is also why a large enough viewport can show the same
    /// tile twice.
    #[must_use]
    pub fn coord_at(&self, col: u32, row: u32) -> Coord {
        let hex = offset_hex(i32_of(col), i32_of(row));
        Coord::new(
            i64::from(self.origin.q()) + i64::from(hex.x),
            i64::from(self.origin.r()) + i64::from(hex.y),
        )
    }

    /// The pixel center of one offset cell.
    #[must_use]
    pub fn pixel_center(&self, col: u32, row: u32) -> (f32, f32) {
        let center = self
            .layout
            .hex_to_world_pos(offset_hex(i32_of(col), i32_of(row)));
        (center.x, center.y)
    }

    /// The pixel holding one offset cell's center.
    ///
    /// The companion to [`Viewport::cell_at_pixel`]: a center pixel always hit
    /// tests back to the cell it came from, which is the rasterizer's
    /// correctness condition and what the render goldens probe.
    #[must_use]
    pub fn center_pixel(&self, col: u32, row: u32) -> (u32, u32) {
        let (x, y) = self.pixel_center(col, row);
        (floor_to_u32(x), floor_to_u32(y))
    }

    /// Which offset cell a pixel belongs to, if any.
    ///
    /// Hit testing, which is one of the things `hexx` is here for. The result is
    /// an offset cell rather than a coordinate: a pixel is turned back into a
    /// tile by integer arithmetic in [`Viewport::coord_at`], never by rounding a
    /// world position into a [`Coord`].
    #[must_use]
    pub fn cell_at_pixel(&self, x: u32, y: u32) -> Option<(u32, u32)> {
        // Sample the pixel's center, not its corner, so a hex boundary running
        // exactly along a pixel edge does not decide by rounding.
        let point = Vec2::new(f32_of(x) + 0.5, f32_of(y) + 0.5);
        let [col, row] = self
            .layout
            .world_pos_to_hex(point)
            .to_offset_coordinates(OFFSET_MODE, ORIENTATION);
        if col < 0 || row < 0 {
            return None;
        }
        let (col, row) = (u32_of(col), u32_of(row));
        (col < self.cols && row < self.rows).then_some((col, row))
    }

    /// Which tile a pixel belongs to, if any.
    #[must_use]
    pub fn locate(&self, x: u32, y: u32) -> Option<Coord> {
        self.cell_at_pixel(x, y)
            .map(|(col, row)| self.coord_at(col, row))
    }
}

/// A rendered image: tightly packed 8-bit RGBA rows, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    /// Image dimensions in pixels.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The RGBA buffer, four bytes per pixel.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// One pixel, or `None` outside the image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let start = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.rgba[start],
            self.rgba[start + 1],
            self.rgba[start + 2],
            self.rgba[start + 3],
        ])
    }
}

/// Renders one scalar layer of a viewport.
///
/// Tiles are sampled in ascending `(col, row)` order — a total order over the
/// viewport's cells — and the result is a color table consulted once per pixel.
/// Fixing the order matters as soon as anything overlaps: section 29 requires a
/// stable order so that edges and labels cannot make output depend on traversal.
///
/// Pixels are assigned by hit testing rather than by filling polygons, so every
/// pixel belongs to exactly one tile and no seam or double-covered pixel is
/// possible between neighbors. Pixel coordinates are consumed here and never
/// reach the generator: sampling happens through [`Viewport::coord_at`], which
/// reads no pixel at all.
#[must_use]
pub fn render(generator: &Generator, viewport: &Viewport, layer: Layer) -> Image {
    let (cols, rows) = viewport.tile_counts();
    let mut colors = vec![BACKGROUND; cols as usize * rows as usize];
    for col in 0..cols {
        for row in 0..rows {
            let value = layer.value(generator, viewport.coord_at(col, row));
            colors[col as usize * rows as usize + row as usize] = color(value);
        }
    }

    let (width, height) = viewport.image_size();
    let mut rgba = vec![0_u8; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let pixel = match viewport.cell_at_pixel(x, y) {
                Some((col, row)) => colors[col as usize * rows as usize + row as usize],
                None => BACKGROUND,
            };
            let start = (y as usize * width as usize + x as usize) * 4;
            rgba[start..start + 4].copy_from_slice(&pixel);
        }
    }

    Image {
        width,
        height,
        rgba,
    }
}

/// Encodes an image as a PNG.
///
/// # Errors
///
/// Returns [`RenderError::Encode`] if the `png` encoder rejects the buffer.
///
/// Note for tests: compare decoded RGBA buffers, never these bytes. The `png`
/// crate's default filter strategy and compression level can change between
/// versions, producing different files for identical images.
pub fn encode_png(image: &Image) -> Result<Vec<u8>, RenderError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&image.rgba)?;
        writer.finish()?;
    }
    Ok(out)
}

/// The `hexx` hex for one offset cell of a viewport.
fn offset_hex(col: i32, row: i32) -> Hex {
    Hex::from_offset_coordinates([col, row], OFFSET_MODE, ORIENTATION)
}

/// `u32` to `i32` for values a validated viewport can produce.
#[expect(
    clippy::cast_possible_wrap,
    reason = "viewport tile counts are bounded well below i32::MAX by the image size gate"
)]
const fn i32_of(value: u32) -> i32 {
    value as i32
}

/// `i32` to `u32`, for a value already checked to be non-negative.
#[expect(
    clippy::cast_sign_loss,
    reason = "callers test for negative before converting"
)]
const fn u32_of(value: i32) -> u32 {
    debug_assert!(value >= 0);
    value as u32
}

/// `u32` to `f32` for a pixel index.
#[expect(
    clippy::cast_precision_loss,
    reason = "pixel indices are bounded by MAX_IMAGE_EDGE, exactly representable in f32"
)]
fn f32_of(value: u32) -> f32 {
    value as f32
}

/// Floors a pixel position to the pixel index containing it.
///
/// Positions below zero clamp to zero: a viewport places every hex inside the
/// image it computed, so a negative here would be a bug rather than a case to
/// handle, and clamping keeps it from becoming an unsigned wrap.
fn floor_to_u32(value: f32) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped into u32 range on the line above"
    )]
    let pixel = value.floor().clamp(0.0, 4_294_967_000.0) as u32;
    pixel
}

/// Rounds a pixel extent up to a whole number of pixels.
fn ceil_to_u32(value: f32) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to a non-negative value inside u32 range on the line above"
    )]
    let pixels = value.ceil().clamp(1.0, 4_294_967_000.0) as u32;
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgvb::{DIRECTIONS, Generator};

    const SEED: u64 = 0x1234_5678_9abc_def0;

    fn generator() -> Generator {
        Generator::with_defaults(SEED)
    }

    #[test]
    fn the_adapter_settles_the_axis_rename() {
        let hex = to_hex(3, -7);
        assert_eq!(hex.x, 3);
        assert_eq!(hex.y, -7);
        // Extremes convert losslessly: `Hex` is i32-backed and `Component` is
        // i16.
        assert_eq!(to_hex(Component::MAX, Component::MIN).x, 32_767);
        assert_eq!(to_hex(Component::MAX, Component::MIN).y, -32_768);
    }

    /// Which way the pixel offset from a cell to a neighbor points on screen.
    ///
    /// The image `y` axis increases downward, so a negative `dy` is north.
    fn compass_of(dx: f32, dy: f32) -> &'static str {
        let vertical = if dy < 0.0 { "N" } else { "S" };
        if dx.abs() < 0.001 {
            return vertical;
        }
        match (dx > 0.0, dy < 0.0) {
            (true, true) => "NE",
            (true, false) => "SE",
            (false, true) => "NW",
            (false, false) => "SW",
        }
    }

    #[test]
    fn the_compass_walk_is_clockwise_and_decreases_the_direction_index() {
        // `DESIGN.md` appendix A, *Rotation senses*, pinned rather than
        // described. Two claims live here, and both are about the picture
        // rather than about the world:
        //
        // 1. This layout draws the admin frame, whose north is absolute
        //    direction 2. Nothing rotates; north is direction 2 because the
        //    flat-top layout puts `(0, -1)` at the top of the image.
        // 2. Reading the compass clockwise — N, NE, SE, S, SW, NW — walks the
        //    direction index *backwards*, because index order is
        //    counter-clockwise as a viewer sees it.
        //
        // A layout change that mirrored the image would keep every golden
        // pixel test passing while making every heading the game prints point
        // the wrong way. This is the test that would fail.
        let expected: [(&str, i32); 6] = [
            ("N", 2),
            ("NE", 1),
            ("SE", 0),
            ("S", 5),
            ("SW", 4),
            ("NW", 3),
        ];

        // An odd window so the centre cell is exact, and both column parities
        // are exercised by running the check at two adjacent centres: the
        // even-`q` offset scheme shoves alternate columns down, so the
        // *offset* neighbors of a cell depend on its column parity even though
        // the axial ones do not.
        let viewport = Viewport::new(Coord::new(0, 0), 9, 9, 8.0).expect("a valid viewport");
        for (centre_col, centre_row) in [(4_u32, 4_u32), (5, 4)] {
            let centre = viewport.coord_at(centre_col, centre_row);
            let (cx, cy) = viewport.pixel_center(centre_col, centre_row);

            for (compass, direction) in expected {
                let neighbor = centre.neighbor(direction);
                let (col, row) = (0..9)
                    .flat_map(|col| (0..9).map(move |row| (col, row)))
                    .find(|(col, row)| viewport.coord_at(*col, *row) == neighbor)
                    .expect("every neighbor of the centre is inside a nine-by-nine window");
                let (nx, ny) = viewport.pixel_center(col, row);
                assert_eq!(
                    compass_of(nx - cx, ny - cy),
                    compass,
                    "direction {direction} from ({centre_col}, {centre_row}) \
                     is not {compass} on screen"
                );
            }
        }

        // The walk itself: clockwise on screen is one step back through the
        // index, six times, returning where it started.
        for window in expected.windows(2) {
            let (from, to) = (window[0].1, window[1].1);
            assert_eq!(
                to,
                (from - 1).rem_euclid(6),
                "the compass walk does not decrease the index"
            );
        }
        assert_eq!(
            (expected[5].1 - 1).rem_euclid(6),
            expected[0].1,
            "the compass walk does not close"
        );
    }

    #[test]
    fn the_two_direction_tables_run_in_opposite_senses() {
        // `hexx_index = (-d).rem_euclid(6)`. The tables agree only at 0 and 3,
        // so nothing may assume they agree.
        for (direction, (dq, dr)) in DIRECTIONS.into_iter().enumerate() {
            // `(-d).rem_euclid(6)`, written for an unsigned index.
            let index = (6 - direction) % 6;
            assert_eq!(
                Hex::NEIGHBORS_COORDS[index],
                to_hex(dq, dr),
                "WGVB direction {direction}"
            );
        }
        assert_eq!(
            Hex::NEIGHBORS_COORDS[0],
            to_hex(DIRECTIONS[0].0, DIRECTIONS[0].1)
        );
        assert_eq!(
            Hex::NEIGHBORS_COORDS[3],
            to_hex(DIRECTIONS[3].0, DIRECTIONS[3].1)
        );
        assert_ne!(
            Hex::NEIGHBORS_COORDS[1],
            to_hex(DIRECTIONS[1].0, DIRECTIONS[1].1)
        );
    }

    #[test]
    fn direction_two_points_to_the_top_of_the_image() {
        // The composition this module's comment describes: flat-top layout,
        // opposite cyclic senses, and a PNG whose `y` grows downward. Assert it
        // on real pixel positions rather than reasoning about it.
        let viewport = Viewport::new(Coord::new(0, 0), 5, 5, 12.0).expect("valid viewport");
        let center = viewport.pixel_center(2, 2);
        let center_coord = viewport.coord_at(2, 2);

        // Find the offset cell holding each neighbor of the center tile.
        let mut found = [(0.0_f32, 0.0_f32); 6];
        for (index, direction) in (0..6_i32).enumerate() {
            let wanted = center_coord.neighbor(direction);
            let mut hit = None;
            for col in 0..5 {
                for row in 0..5 {
                    if viewport.coord_at(col, row) == wanted {
                        hit = Some(viewport.pixel_center(col, row));
                    }
                }
            }
            found[index] = hit.expect("every neighbor is inside a 5x5 viewport");
        }

        // Direction 2 is straight up: same x, smaller y.
        assert!((found[2].0 - center.0).abs() < 0.01, "{:?}", found[2]);
        assert!(found[2].1 < center.1, "direction 2 is not above the center");
        // Direction 5 is straight down, its opposite.
        assert!((found[5].0 - center.0).abs() < 0.01, "{:?}", found[5]);
        assert!(found[5].1 > center.1, "direction 5 is not below the center");
        // Direction 0 is to the right, direction 3 to the left.
        assert!(found[0].0 > center.0);
        assert!(found[3].0 < center.0);
    }

    #[test]
    fn a_viewport_rejects_impossible_requests() {
        assert!(matches!(
            Viewport::new(Coord::ORIGIN, 0, 4, 8.0),
            Err(RenderError::EmptyViewport { cols: 0, rows: 4 })
        ));
        assert!(matches!(
            Viewport::new(Coord::ORIGIN, 4, 0, 8.0),
            Err(RenderError::EmptyViewport { cols: 4, rows: 0 })
        ));
        for bad in [0.0, 0.5, -3.0, f32::NAN, f32::INFINITY] {
            assert!(
                matches!(
                    Viewport::new(Coord::ORIGIN, 4, 4, bad),
                    Err(RenderError::HexRadius(_))
                ),
                "radius {bad}"
            );
        }
        assert!(matches!(
            Viewport::new(Coord::ORIGIN, 100_000, 100_000, 8.0),
            Err(RenderError::TooLarge { .. })
        ));
    }

    #[test]
    fn a_centered_viewport_puts_the_requested_tile_in_the_center_cell() {
        // Every odd pair the constructor accepts, over a range of centers that
        // includes both column parities, negative components, and coordinates
        // far enough out that normalization is doing work.
        for center in [
            Coord::new(0, 0),
            Coord::new(1, 0),
            Coord::new(-87, 6543),
            Coord::new(12_345, -20_000),
            Coord::new(32_767, 0),
        ] {
            for cols in [1_u32, 3, 5, 61, 101] {
                for rows in [1_u32, 3, 5, 45, 99] {
                    let viewport = Viewport::centered_on(center, cols, rows, 4.0)
                        .expect("an odd window is accepted");
                    assert_eq!(
                        viewport.coord_at(cols / 2, rows / 2),
                        center,
                        "{cols} x {rows} centered on ({}, {})",
                        center.q(),
                        center.r()
                    );
                    assert_eq!(viewport.center(), Some(center));
                    assert_eq!(viewport.tile_counts(), (cols, rows));
                }
            }
        }
    }

    #[test]
    fn a_centered_viewport_rejects_a_window_with_no_center_cell() {
        assert!(matches!(
            Viewport::centered_on(Coord::ORIGIN, 60, 45, 8.0),
            Err(RenderError::EvenViewport { cols: 60, rows: 45 })
        ));
        assert!(matches!(
            Viewport::centered_on(Coord::ORIGIN, 61, 44, 8.0),
            Err(RenderError::EvenViewport { cols: 61, rows: 44 })
        ));
        // Zero is even, but "empty" is the more useful complaint.
        assert!(matches!(
            Viewport::centered_on(Coord::ORIGIN, 0, 45, 8.0),
            Err(RenderError::EmptyViewport { cols: 0, rows: 45 })
        ));
        assert!(matches!(
            Viewport::centered_on(Coord::ORIGIN, 61, 0, 8.0),
            Err(RenderError::EmptyViewport { cols: 61, rows: 0 })
        ));
        // The gates `Viewport::new` owns still fire through this constructor.
        assert!(matches!(
            Viewport::centered_on(Coord::ORIGIN, 61, 45, 0.5),
            Err(RenderError::HexRadius(_))
        ));
        assert!(matches!(
            Viewport::centered_on(Coord::ORIGIN, 100_001, 100_001, 8.0),
            Err(RenderError::TooLarge { .. })
        ));
    }

    #[test]
    fn an_even_window_has_no_center_cell_to_report() {
        let viewport = Viewport::new(Coord::ORIGIN, 60, 45, 8.0).expect("a valid viewport");
        assert_eq!(viewport.center(), None);
        let viewport = Viewport::new(Coord::ORIGIN, 61, 44, 8.0).expect("a valid viewport");
        assert_eq!(viewport.center(), None);
    }

    #[test]
    fn centering_and_reading_the_center_are_inverse() {
        // The round trip the viewer depends on: a link names a center, the
        // window drawn from it reports the same center, and the window's first
        // tile agrees with `coord_at` for every cell.
        let viewport =
            Viewport::centered_on(Coord::new(-87, 6543), 61, 45, 10.0).expect("a valid viewport");
        let rebuilt = Viewport::new(viewport.origin(), 61, 45, 10.0).expect("a valid viewport");
        assert_eq!(rebuilt.center(), Some(Coord::new(-87, 6543)));
        for col in 0..61 {
            for row in 0..45 {
                assert_eq!(viewport.coord_at(col, row), rebuilt.coord_at(col, row));
            }
        }
    }

    #[test]
    fn every_tile_in_the_viewport_has_a_distinct_offset_cell() {
        let viewport = Viewport::new(Coord::new(-40, 17), 9, 7, 6.0).expect("valid viewport");
        let mut seen = Vec::new();
        for col in 0..9 {
            for row in 0..7 {
                seen.push(viewport.coord_at(col, row));
            }
        }
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count);
    }

    #[test]
    fn hit_testing_round_trips_through_every_hex_center() {
        // The rasterizer's correctness condition: the pixel at a tile's center
        // must be attributed to that tile.
        let viewport = Viewport::new(Coord::new(11, -5), 12, 9, 7.0).expect("valid viewport");
        for col in 0..12 {
            for row in 0..9 {
                let (px, py) = viewport.center_pixel(col, row);
                assert_eq!(
                    viewport.cell_at_pixel(px, py),
                    Some((col, row)),
                    "center of ({col}, {row}) at pixel ({px}, {py})"
                );
                assert_eq!(viewport.locate(px, py), Some(viewport.coord_at(col, row)));
            }
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        let generator = generator();
        let viewport = Viewport::new(Coord::new(-13, 29), 10, 8, 5.0).expect("valid viewport");
        for layer in Layer::ALL {
            let first = render(&generator, &viewport, layer);
            let second = render(&generator, &viewport, layer);
            assert_eq!(first, second, "{}", layer.name());
        }
    }

    #[test]
    fn the_image_is_the_size_the_viewport_promised() {
        let generator = generator();
        for (cols, rows, radius) in [(1_u32, 1_u32, 4.0_f32), (3, 2, 9.0), (17, 11, 3.0)] {
            let viewport =
                Viewport::new(Coord::ORIGIN, cols, rows, radius).expect("valid viewport");
            let image = render(&generator, &viewport, Layer::ElevationRaw);
            assert_eq!(image.size(), viewport.image_size());
            let (w, h) = image.size();
            assert_eq!(image.rgba().len(), w as usize * h as usize * 4);
        }
    }

    #[test]
    fn every_hex_center_pixel_carries_its_own_tile_color() {
        let generator = generator();
        let viewport = Viewport::new(Coord::new(500, -300), 8, 6, 10.0).expect("valid viewport");
        for layer in Layer::ALL {
            let image = render(&generator, &viewport, layer);
            for col in 0..8 {
                for row in 0..6 {
                    let (px, py) = viewport.center_pixel(col, row);
                    let coord = viewport.coord_at(col, row);
                    assert_eq!(
                        image.pixel(px, py),
                        Some(color(layer.value(&generator, coord))),
                        "{} at ({col}, {row})",
                        layer.name()
                    );
                }
            }
        }
    }

    #[test]
    fn the_whole_image_is_covered_and_mostly_by_tiles() {
        // Every pixel is written: no gap between neighboring hexes, and no
        // uninitialized buffer. Background is confined to the ragged edges.
        let generator = generator();
        let viewport = Viewport::new(Coord::ORIGIN, 20, 16, 8.0).expect("valid viewport");
        let image = render(&generator, &viewport, Layer::ElevationRaw);
        let (width, height) = image.size();
        let mut background = 0_u32;
        for y in 0..height {
            for x in 0..width {
                let pixel = image.pixel(x, y).expect("inside the image");
                assert_eq!(pixel[3], 255, "pixel ({x}, {y}) is not opaque");
                background += u32::from(pixel == BACKGROUND);
            }
        }
        let share = f64::from(background) / f64::from(width * height);
        assert!(share < 0.2, "background covers {share} of the image");
    }

    #[test]
    fn different_layers_produce_different_images() {
        let generator = generator();
        let viewport = Viewport::new(Coord::new(77, 77), 12, 10, 6.0).expect("valid viewport");
        let images: Vec<Image> = Layer::ALL
            .into_iter()
            .map(|layer| render(&generator, &viewport, layer))
            .collect();
        for i in 0..images.len() {
            for j in (i + 1)..images.len() {
                assert_ne!(
                    images[i],
                    images[j],
                    "{} and {} rendered identically",
                    Layer::ALL[i].name(),
                    Layer::ALL[j].name()
                );
            }
        }
    }

    #[test]
    fn distant_windows_differ() {
        // The point of an unbounded world. Two windows a long way apart must
        // not render the same picture.
        let generator = generator();
        let a = Viewport::new(Coord::new(-20_000, 9_000), 10, 8, 5.0).expect("valid viewport");
        let b = Viewport::new(Coord::new(20_000, -9_000), 10, 8, 5.0).expect("valid viewport");
        assert_ne!(
            render(&generator, &a, Layer::ElevationRaw),
            render(&generator, &b, Layer::ElevationRaw)
        );
    }

    #[test]
    fn layer_names_round_trip_and_unknown_names_are_rejected() {
        for layer in Layer::ALL {
            assert_eq!(Layer::parse(layer.name()).expect("known layer"), layer);
        }
        assert!(matches!(
            Layer::parse("terrain"),
            Err(RenderError::UnknownLayer(name)) if name == "terrain"
        ));
        assert!(Layer::parse("Continentalness").is_err());
    }

    #[test]
    fn png_round_trips_through_a_decoder_pixel_for_pixel() {
        // Section 29: golden-compare decoded RGBA buffers, not PNG file bytes.
        // This test is the reason that rule is usable — it proves the encoder
        // preserves the buffer, so a pixel comparison is a comparison of what
        // we actually care about.
        let generator = generator();
        let viewport = Viewport::new(Coord::new(-3, 4), 9, 7, 6.0).expect("valid viewport");
        let image = render(&generator, &viewport, Layer::ElevationRaw);
        let bytes = encode_png(&image).expect("encoding succeeds");

        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let mut reader = decoder.read_info().expect("a readable PNG");
        let mut decoded = vec![0_u8; reader.output_buffer_size().expect("a bounded buffer")];
        let info = reader.next_frame(&mut decoded).expect("one frame");

        assert_eq!(info.width, image.size().0);
        assert_eq!(info.height, image.size().1);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        assert_eq!(&decoded[..info.buffer_size()], image.rgba());
    }

    #[test]
    fn a_viewport_that_wraps_the_world_shows_the_wrapped_tiles() {
        // Rendering is bounded, but the world is not, and a window placed on an
        // edge must show the tiles wrapping does put there rather than failing.
        let generator = generator();
        let edge = Coord::new(i64::from(wgvb::Component::MAX) - 2, 0);
        let viewport = Viewport::new(edge, 8, 6, 5.0).expect("valid viewport");
        let image = render(&generator, &viewport, Layer::ElevationRaw);
        assert_eq!(image.size(), viewport.image_size());

        // Cells past the `+q` edge are canonical coordinates somewhere else
        // entirely, and `coord_at` gets there by normalizing in `Coord` rather
        // than by anything `hexx` did.
        let far = offset_hex(7, 0);
        let unwrapped_q = i64::from(edge.q()) + i64::from(far.x);
        assert!(unwrapped_q > i64::from(wgvb::Component::MAX));
        assert_eq!(
            viewport.coord_at(7, 0),
            Coord::new(unwrapped_q, i64::from(edge.r()) + i64::from(far.y))
        );
        assert!(i64::from(viewport.coord_at(7, 0).q()) < unwrapped_q);
    }
}
