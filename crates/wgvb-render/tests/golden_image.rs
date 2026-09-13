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
//! # Recorded for algorithm version 2
//!
//! Every row moved when phase 4 bumped `ALGORITHM_VERSION` — see the
//! compatibility note in `wgvb/tests/golden.rs` — and four rows are new
//! layers. `RENDER_VERSION` did **not** move: the renderer draws the same
//! palette through the same layout, and what changed is the world underneath
//! it. That distinction is the reason the two versions are separate numbers.
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
        [36, 96, 158, 255], [36, 94, 156, 255], [35, 93, 155, 255], [34, 92, 154, 255],
        [37, 97, 159, 255], [36, 95, 157, 255], [35, 94, 156, 255], [35, 93, 155, 255],
        [37, 97, 158, 255], [36, 95, 157, 255], [35, 94, 156, 255], [34, 93, 155, 255],
        [38, 98, 159, 255], [37, 96, 158, 255], [36, 95, 157, 255], [35, 94, 156, 255],
        [38, 98, 159, 255], [37, 96, 158, 255], [36, 95, 157, 255], [35, 93, 155, 255],
    ]),
    // regional
    (Layer::Regional, [
        [135, 120, 70, 255], [133, 121, 70, 255], [132, 121, 70, 255], [131, 122, 70, 255],
        [135, 120, 70, 255], [133, 121, 70, 255], [132, 122, 70, 255], [131, 122, 70, 255],
        [133, 121, 70, 255], [131, 122, 70, 255], [130, 122, 70, 255], [129, 122, 70, 255],
        [133, 121, 70, 255], [131, 122, 70, 255], [129, 122, 70, 255], [128, 123, 70, 255],
        [131, 122, 70, 255], [129, 123, 70, 255], [127, 123, 70, 255], [127, 124, 70, 255],
    ]),
    // local
    (Layer::Local, [
        [15, 57, 120, 255], [13, 52, 113, 255], [13, 52, 112, 255], [23, 73, 138, 255],
        [13, 51, 110, 255], [14, 52, 113, 255], [11, 44, 100, 255], [12, 45, 102, 255],
        [14, 55, 118, 255], [14, 54, 116, 255], [15, 57, 120, 255], [14, 55, 118, 255],
        [14, 56, 118, 255], [16, 61, 126, 255], [16, 60, 125, 255], [13, 49, 108, 255],
        [20, 69, 134, 255], [27, 79, 143, 255], [12, 46, 103, 255], [15, 57, 120, 255],
    ]),
    // detail
    (Layer::Detail, [
        [88, 157, 201, 255], [108, 146, 81, 255], [97, 135, 70, 255], [102, 133, 70, 255],
        [46, 111, 171, 255], [94, 162, 204, 255], [147, 163, 103, 255], [94, 136, 70, 255],
        [40, 101, 162, 255], [54, 125, 183, 255], [110, 177, 211, 255], [103, 144, 79, 255],
        [41, 103, 164, 255], [48, 116, 175, 255], [107, 174, 210, 255], [213, 201, 158, 255],
        [29, 84, 147, 255], [54, 124, 183, 255], [173, 174, 117, 255], [91, 139, 72, 255],
    ]),
    // elevation-raw
    (Layer::ElevationRaw, [
        [60, 131, 188, 255], [56, 128, 186, 255], [56, 127, 185, 255], [60, 132, 188, 255],
        [56, 128, 186, 255], [56, 128, 186, 255], [55, 126, 184, 255], [55, 126, 184, 255],
        [56, 128, 186, 255], [55, 127, 185, 255], [55, 127, 185, 255], [55, 126, 185, 255],
        [58, 130, 187, 255], [56, 128, 186, 255], [56, 128, 186, 255], [54, 125, 183, 255],
        [58, 130, 187, 255], [61, 133, 188, 255], [54, 125, 183, 255], [55, 126, 184, 255],
    ]),
    // elevation
    (Layer::Elevation, [
        [18, 65, 131, 255], [17, 64, 129, 255], [17, 63, 129, 255], [17, 64, 130, 255],
        [17, 64, 130, 255], [17, 63, 129, 255], [16, 61, 127, 255], [16, 62, 127, 255],
        [17, 63, 129, 255], [16, 62, 128, 255], [16, 62, 128, 255], [16, 62, 128, 255],
        [18, 65, 131, 255], [17, 64, 129, 255], [16, 63, 128, 255], [16, 61, 126, 255],
        [17, 64, 130, 255], [18, 65, 130, 255], [16, 61, 127, 255], [16, 62, 128, 255],
    ]),
    // relief
    (Layer::Relief, [
        [96, 135, 70, 255], [95, 135, 70, 255], [101, 133, 70, 255], [98, 134, 70, 255],
        [124, 124, 70, 255], [93, 136, 70, 255], [95, 135, 70, 255], [98, 134, 70, 255],
        [105, 132, 70, 255], [91, 137, 70, 255], [138, 159, 98, 255], [97, 135, 70, 255],
        [112, 129, 70, 255], [90, 137, 70, 255], [103, 132, 70, 255], [104, 132, 70, 255],
        [124, 153, 90, 255], [103, 132, 70, 255], [106, 131, 70, 255], [110, 130, 70, 255],
    ]),
    // ridge
    (Layer::Ridge, [
        [98, 166, 206, 255], [73, 144, 194, 255], [86, 155, 200, 255], [91, 160, 202, 255],
        [199, 189, 140, 255], [80, 149, 197, 255], [63, 134, 189, 255], [81, 151, 198, 255],
        [146, 188, 198, 255], [73, 143, 194, 255], [76, 146, 195, 255], [102, 169, 207, 255],
        [156, 166, 108, 255], [114, 180, 213, 255], [92, 160, 203, 255], [106, 173, 209, 255],
        [169, 172, 116, 255], [205, 195, 149, 255], [116, 182, 214, 255], [203, 192, 145, 255],
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
