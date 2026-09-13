//! Golden rendered output for render version 1.
//!
//! `DESIGN.md` section 29 and the issue for phase 2.
//!
//! # Decoded pixels, never file bytes
//!
//! The `png` crate's default filter strategy and compression level can change
//! between versions, producing a different file for an identical image. So this
//! golden records *pixels*: the image is rendered, encoded, decoded again, and
//! the decoded buffer is what the table below is compared against. A change to
//! the encoder's settings does not move these values; a change to the palette,
//! the layout, or the generator does.
//!
//! # What the probes are
//!
//! One pixel at the center of every tile in a small window, for every layer.
//! Centers are chosen deliberately: they sit far from any hex boundary, so the
//! table records what the renderer decided rather than how a boundary pixel
//! rounded. Boundary behavior is covered by the unit tests in `wgvb-render`,
//! which check that every pixel is attributed to exactly one tile.
//!
//! # The nearly flat rows
//!
//! The `region-influence` and `roughness` rows differ from one another by a
//! single palette step across the whole window, because a five-by-four window
//! is twenty tiles out of a 128-hex region: those layers are *supposed* to be
//! almost flat at this scale, and a window where they were not would mean the
//! blend had collapsed toward per-tile noise. What those rows pin is that the
//! near-flat value is the recorded one.
//!
//! # Recorded for algorithm version 3
//!
//! Every row that reads a noise field moved when the composition gained
//! per-field octave counts and a seed-derived sampling offset — see the
//! compatibility note in `wgvb/tests/golden.rs`. The `roughness` and
//! `region-influence` rows did not move at all, and that is worth noticing
//! rather than glossing: those two come from the region hierarchy, which hashes
//! anchor addresses and never samples a continuous field, so a change to the
//! noise cannot reach them. A change that moved them too would have meant the
//! two systems were entangled somewhere they are documented not to be. `RENDER_VERSION` did **not** move: the renderer
//! draws the same palette through the same layout, and what changed is the
//! world underneath it. That distinction is the reason the two versions are
//! separate numbers.
//!
//! # If these values move
//!
//! Rendered output changed. Either bump `RENDER_VERSION` and re-record
//! deliberately, saying so in the commit message, or fix the change that moved
//! them. If the *generator* moved, the golden coordinates in `wgvb` will have
//! failed too, and that is the more serious of the two.

use wgvb::{Coord, Generator};
use wgvb_render::{BACKGROUND, Image, Layer, Viewport, encode_png, render};

/// The seed, window, and scale the table was recorded with.
const GOLDEN_SEED: u64 = 0x0123_4567_89ab_cdef;
const GOLDEN_ORIGIN: (i64, i64) = (-40, 25);
const GOLDEN_COLS: u32 = 5;
const GOLDEN_ROWS: u32 = 4;
const GOLDEN_HEX_RADIUS: f32 = 6.0;

/// The image those settings produce.
const GOLDEN_SIZE: (u32, u32) = (48, 47);

/// Tile-center colors, in ascending `(col, row)` order — the same order the
/// renderer walks the viewport in.
#[rustfmt::skip]
const GOLDEN: [(Layer, [[u8; 4]; 20]); 10] = [
    // continentalness
    (Layer::Continentalness, [
        [108, 175, 210, 255], [114, 180, 213, 255], [214, 202, 160, 255], [206, 196, 150, 255],
        [108, 175, 210, 255], [114, 180, 213, 255], [214, 202, 159, 255], [206, 195, 150, 255],
        [114, 180, 213, 255], [213, 202, 159, 255], [206, 195, 149, 255], [198, 189, 140, 255],
        [114, 180, 213, 255], [213, 201, 159, 255], [206, 195, 149, 255], [198, 189, 139, 255],
        [213, 201, 159, 255], [206, 195, 149, 255], [198, 188, 139, 255], [190, 182, 129, 255],
    ]),
    // regional
    (Layer::Regional, [
        [14, 55, 117, 255], [14, 55, 117, 255], [14, 55, 117, 255], [14, 56, 118, 255],
        [14, 55, 118, 255], [14, 56, 118, 255], [14, 56, 118, 255], [15, 56, 119, 255],
        [15, 56, 119, 255], [15, 57, 120, 255], [15, 57, 120, 255], [15, 58, 121, 255],
        [15, 57, 121, 255], [15, 58, 121, 255], [15, 58, 122, 255], [15, 59, 123, 255],
        [15, 59, 123, 255], [15, 59, 123, 255], [15, 60, 124, 255], [16, 60, 126, 255],
    ]),
    // local
    (Layer::Local, [
        [39, 100, 162, 255], [93, 161, 203, 255], [207, 196, 151, 255], [137, 159, 98, 255],
        [33, 90, 153, 255], [48, 114, 174, 255], [64, 135, 190, 255], [214, 202, 159, 255],
        [35, 93, 155, 255], [44, 108, 169, 255], [74, 144, 194, 255], [205, 194, 148, 255],
        [22, 73, 137, 255], [34, 92, 154, 255], [45, 110, 170, 255], [71, 142, 193, 255],
        [32, 88, 151, 255], [44, 108, 169, 255], [54, 125, 183, 255], [190, 197, 173, 255],
    ]),
    // detail
    (Layer::Detail, [
        [127, 154, 92, 255], [131, 156, 94, 255], [111, 148, 83, 255], [116, 150, 86, 255],
        [152, 165, 106, 255], [150, 164, 105, 255], [119, 151, 88, 255], [108, 147, 81, 255],
        [131, 156, 94, 255], [88, 138, 70, 255], [91, 137, 70, 255], [127, 155, 92, 255],
        [88, 138, 70, 255], [97, 134, 70, 255], [102, 133, 70, 255], [91, 137, 70, 255],
        [101, 133, 70, 255], [103, 133, 70, 255], [99, 134, 70, 255], [100, 143, 77, 255],
    ]),
    // elevation-raw
    (Layer::ElevationRaw, [
        [46, 112, 172, 255], [49, 116, 175, 255], [50, 118, 177, 255], [51, 120, 179, 255],
        [46, 111, 171, 255], [47, 114, 174, 255], [49, 117, 176, 255], [51, 119, 178, 255],
        [47, 113, 173, 255], [48, 115, 175, 255], [50, 119, 178, 255], [52, 121, 180, 255],
        [46, 112, 172, 255], [48, 115, 174, 255], [50, 118, 177, 255], [51, 120, 179, 255],
        [48, 115, 175, 255], [50, 118, 177, 255], [51, 120, 179, 255], [53, 123, 182, 255],
    ]),
    // elevation
    (Layer::Elevation, [
        [14, 54, 115, 255], [14, 55, 118, 255], [14, 56, 119, 255], [15, 57, 120, 255],
        [14, 54, 115, 255], [14, 55, 116, 255], [14, 55, 118, 255], [15, 57, 120, 255],
        [14, 54, 116, 255], [14, 55, 117, 255], [15, 57, 119, 255], [15, 58, 121, 255],
        [14, 54, 115, 255], [14, 55, 117, 255], [15, 56, 119, 255], [15, 58, 121, 255],
        [14, 55, 118, 255], [15, 57, 120, 255], [15, 58, 122, 255], [15, 59, 124, 255],
    ]),
    // relief
    (Layer::Relief, [
        [94, 141, 74, 255], [97, 135, 70, 255], [90, 137, 70, 255], [95, 136, 70, 255],
        [107, 146, 80, 255], [93, 136, 70, 255], [94, 136, 70, 255], [96, 135, 70, 255],
        [88, 138, 70, 255], [96, 135, 70, 255], [100, 133, 70, 255], [97, 135, 70, 255],
        [91, 137, 70, 255], [101, 133, 70, 255], [109, 130, 70, 255], [105, 132, 70, 255],
        [107, 131, 70, 255], [113, 129, 70, 255], [120, 126, 70, 255], [129, 123, 70, 255],
    ]),
    // ridge
    (Layer::Ridge, [
        [130, 112, 71, 255], [133, 121, 70, 255], [120, 126, 70, 255], [111, 129, 70, 255],
        [129, 110, 71, 255], [134, 121, 70, 255], [117, 127, 70, 255], [110, 130, 70, 255],
        [136, 120, 70, 255], [118, 127, 70, 255], [111, 130, 70, 255], [106, 131, 70, 255],
        [134, 118, 70, 255], [122, 125, 70, 255], [113, 129, 70, 255], [110, 130, 70, 255],
        [125, 124, 70, 255], [115, 128, 70, 255], [112, 129, 70, 255], [111, 129, 70, 255],
    ]),
    // roughness
    (Layer::Roughness, [
        [29, 84, 147, 255], [30, 84, 148, 255], [30, 85, 148, 255], [30, 85, 148, 255],
        [29, 84, 147, 255], [29, 84, 147, 255], [29, 84, 147, 255], [30, 84, 148, 255],
        [29, 84, 147, 255], [29, 84, 147, 255], [29, 84, 147, 255], [30, 84, 148, 255],
        [29, 83, 147, 255], [29, 84, 147, 255], [29, 84, 147, 255], [29, 84, 147, 255],
        [29, 83, 147, 255], [29, 83, 147, 255], [29, 84, 147, 255], [29, 84, 147, 255],
    ]),
    // region-influence
    (Layer::RegionInfluence, [
        [42, 105, 166, 255], [42, 105, 166, 255], [42, 105, 166, 255], [42, 105, 166, 255],
        [42, 104, 165, 255], [42, 104, 165, 255], [42, 105, 165, 255], [42, 105, 166, 255],
        [41, 104, 165, 255], [42, 104, 165, 255], [42, 104, 165, 255], [42, 104, 165, 255],
        [41, 104, 165, 255], [41, 104, 165, 255], [41, 104, 165, 255], [41, 104, 165, 255],
        [41, 103, 164, 255], [41, 103, 164, 255], [41, 104, 165, 255], [41, 104, 165, 255],
    ]),
];

/// Renders the golden window, encodes it, and decodes it back.
///
/// Everything the table is compared against has been through a real PNG
/// round trip, so the golden covers the encoder as well as the renderer.
fn decoded_golden_image(layer: Layer) -> Image {
    let generator = Generator::with_defaults(GOLDEN_SEED);
    let viewport = Viewport::new(
        Coord::new(GOLDEN_ORIGIN.0, GOLDEN_ORIGIN.1),
        GOLDEN_COLS,
        GOLDEN_ROWS,
        GOLDEN_HEX_RADIUS,
    )
    .expect("the golden viewport is valid");
    let image = render(&generator, &viewport, layer);

    let bytes = encode_png(&image).expect("encoding succeeds");
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("a readable PNG");
    let mut buffer = vec![0_u8; reader.output_buffer_size().expect("a bounded buffer")];
    let info = reader.next_frame(&mut buffer).expect("one frame");

    assert_eq!((info.width, info.height), image.size());
    assert_eq!(
        &buffer[..info.buffer_size()],
        image.rgba(),
        "the PNG round trip changed a pixel"
    );
    image
}

/// The golden viewport, for locating tile centers.
fn golden_viewport() -> Viewport {
    Viewport::new(
        Coord::new(GOLDEN_ORIGIN.0, GOLDEN_ORIGIN.1),
        GOLDEN_COLS,
        GOLDEN_ROWS,
        GOLDEN_HEX_RADIUS,
    )
    .expect("the golden viewport is valid")
}

#[test]
fn the_golden_window_is_the_size_it_was_recorded_at() {
    assert_eq!(golden_viewport().image_size(), GOLDEN_SIZE);
}

#[test]
fn every_layer_matches_its_golden_pixels() {
    let viewport = golden_viewport();
    let mut failures = Vec::new();

    for (layer, expected) in GOLDEN {
        let image = decoded_golden_image(layer);
        assert_eq!(image.size(), GOLDEN_SIZE);

        let mut index = 0;
        for col in 0..GOLDEN_COLS {
            for row in 0..GOLDEN_ROWS {
                let (x, y) = viewport.center_pixel(col, row);
                let actual = image
                    .pixel(x, y)
                    .expect("a tile center is inside the image");
                if actual != expected[index] {
                    failures.push(format!(
                        "{} tile ({col}, {row}) at pixel ({x}, {y}): expected {:?}, got {actual:?}",
                        layer.name(),
                        expected[index]
                    ));
                }
                index += 1;
            }
        }
        assert_eq!(index, expected.len(), "the table is the wrong length");
    }

    assert!(
        failures.is_empty(),
        "{} golden pixel(s) moved. This is a render compatibility change, \
         not a test to re-record:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_corners_of_the_golden_window_are_background() {
    // The ragged edges of a hex grid in a rectangular image. Recording them
    // pins that the image is laid out where it was, not merely that the tiles
    // are right.
    let image = decoded_golden_image(Layer::ElevationRaw);
    let (width, height) = image.size();
    for (x, y) in [
        (0, 0),
        (width - 1, 0),
        (0, height - 1),
        (width - 1, height - 1),
    ] {
        assert_eq!(image.pixel(x, y), Some(BACKGROUND), "corner ({x}, {y})");
    }
}

#[test]
fn the_golden_table_distinguishes_the_layers() {
    // A table where two layers matched would pass forever while proving
    // nothing about either.
    for (index, (layer, pixels)) in GOLDEN.iter().enumerate() {
        for (other_layer, other_pixels) in GOLDEN.iter().skip(index + 1) {
            assert_ne!(
                pixels,
                other_pixels,
                "{} and {} recorded identical pixels",
                layer.name(),
                other_layer.name()
            );
        }
    }
}

#[test]
fn the_golden_table_covers_every_layer_exactly_once() {
    let recorded: Vec<Layer> = GOLDEN.iter().map(|(layer, _)| *layer).collect();
    for layer in Layer::ALL {
        assert_eq!(
            recorded.iter().filter(|l| **l == layer).count(),
            1,
            "{} is not recorded exactly once",
            layer.name()
        );
    }
    assert_eq!(recorded.len(), Layer::ALL.len());
}
