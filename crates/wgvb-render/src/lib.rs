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
/// relief, climate, and terrain arrive with the phases that generate them.
///
/// Every layer reads one scalar out of a [`Sample`], which is what keeps the
/// renderer from needing a second traversal of the world per layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Continentalness,
    Regional,
    Local,
    Detail,
    ElevationRaw,
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
    pub const ALL: [Layer; 6] = [
        Layer::Continentalness,
        Layer::Regional,
        Layer::Local,
        Layer::Detail,
        Layer::ElevationRaw,
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

    /// Reads this layer's scalar out of a sample.
    #[must_use]
    pub const fn value(self, sample: &Sample) -> f64 {
        match self {
            Layer::Continentalness => sample.continentalness,
            Layer::Regional => sample.regional,
            Layer::Local => sample.local,
            Layer::Detail => sample.detail,
            Layer::ElevationRaw => sample.elevation_raw,
            Layer::RegionInfluence => sample.regional_uplift,
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

    /// The tile at the viewport's `(0, 0)` offset cell.
    #[must_use]
    pub const fn origin(&self) -> Coord {
        self.origin
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
            let sample = generator.sample(viewport.coord_at(col, row));
            colors[col as usize * rows as usize + row as usize] = color(layer.value(&sample));
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
                    let sample = generator.sample(viewport.coord_at(col, row));
                    assert_eq!(
                        image.pixel(px, py),
                        Some(color(layer.value(&sample))),
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
