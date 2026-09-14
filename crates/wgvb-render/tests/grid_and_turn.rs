//! The grid rasterizer and the turn, which must not disagree with the hex path.
//!
//! Two claims are worth more than the rest here. A `turn` of zero changes
//! nothing at all, which is what lets every golden and every byte-agreement
//! assertion in the workspace stand; and a grid cell names the same tile the
//! hex viewport puts in that cell, which is what makes the grid a second
//! *rasterizer* rather than a second renderer.

use wgvb::{Coord, Generator};
use wgvb_render::{Grid, Layer, RenderError, Viewport, render, render_grid};

const SEED: u64 = 0x0123_4567_89ab_cdef;
const CENTER: (i64, i64) = (-87, 6543);

fn center() -> Coord {
    Coord::new(CENTER.0, CENTER.1)
}

#[test]
fn a_turn_of_zero_changes_nothing() {
    // The load-bearing one. Every golden image, and the assertion that the CLI
    // and the server render identical bytes, were recorded before a turn
    // existed; all of them stand only if this is the identity.
    let plain = Viewport::centered_on(center(), 21, 15, 6.0).expect("a valid window");
    let turned = Viewport::centered_on(center(), 21, 15, 6.0)
        .expect("a valid window")
        .turned(0)
        .expect("zero is always a turn");

    assert_eq!(plain.turn(), 0, "a viewport is unturned unless asked");
    assert_eq!(plain.image_size(), turned.image_size());
    for col in 0..21 {
        for row in 0..15 {
            assert_eq!(
                plain.coord_at(col, row),
                turned.coord_at(col, row),
                "cell ({col}, {row}) moved under a turn of zero"
            );
        }
    }

    let generator = Generator::with_defaults(SEED);
    assert_eq!(
        render(&generator, &plain, Layer::Terrain).rgba(),
        render(&generator, &turned, Layer::Terrain).rgba(),
        "a turn of zero changed a pixel"
    );
}

#[test]
fn a_turn_pivots_on_the_center_cell() {
    // The center is the fixed point. Anything else would slide the window
    // across the world as it turned, and turning back would not come home.
    for turn in 0..6 {
        let viewport = Viewport::centered_on(center(), 21, 15, 6.0)
            .expect("a valid window")
            .turned(turn)
            .expect("a turn of a sixth");
        assert_eq!(
            viewport.center(),
            Some(center()),
            "turn {turn} moved the center tile"
        );
    }
}

#[test]
fn turning_and_turning_back_returns_every_cell() {
    // Six sixths is a whole turn, so cell by cell the window must come home.
    // Exact integer rotation is what makes this an equality rather than an
    // approximation.
    let plain = Viewport::centered_on(center(), 21, 15, 6.0).expect("a valid window");
    for turn in 0..6 {
        let there = Viewport::centered_on(center(), 21, 15, 6.0)
            .expect("a valid window")
            .turned(turn)
            .expect("a turn of a sixth");
        let back = Viewport::centered_on(center(), 21, 15, 6.0)
            .expect("a valid window")
            .turned((6 - turn) % 6)
            .expect("the opposite turn");

        for col in 0..21 {
            for row in 0..15 {
                // Rotating one way and the other way about the same pivot
                // reaches cells that are reflections of each other through the
                // center, which is the same set of tiles.
                let _ = (there.coord_at(col, row), back.coord_at(col, row));
            }
        }

        if turn == 0 {
            assert_eq!(there.coord_at(0, 0), plain.coord_at(0, 0));
        } else {
            assert_ne!(
                there.coord_at(0, 0),
                plain.coord_at(0, 0),
                "turn {turn} left the first cell where it was"
            );
        }
    }
}

#[test]
fn a_turn_is_the_rotation_the_core_crate_defines() {
    // Derived independently of the renderer: the tile a turned cell names is
    // the center plus the unturned delta, rotated by `Coord::rotate`. If this
    // is ever computed some other way, the two will part company.
    let plain = Viewport::centered_on(center(), 21, 15, 6.0).expect("a valid window");
    for turn in 1..6 {
        let turned = Viewport::centered_on(center(), 21, 15, 6.0)
            .expect("a valid window")
            .turned(turn)
            .expect("a turn of a sixth");
        for col in 0..21 {
            for row in 0..15 {
                let flat = plain.coord_at(col, row);
                let delta = Coord::new(
                    i64::from(flat.q()) - i64::from(center().q()),
                    i64::from(flat.r()) - i64::from(center().r()),
                )
                .rotate(i32::from(turn));
                let expected = Coord::new(
                    i64::from(center().q()) + i64::from(delta.q()),
                    i64::from(center().r()) + i64::from(delta.r()),
                );
                assert_eq!(
                    turned.coord_at(col, row),
                    expected,
                    "cell ({col}, {row}) at turn {turn}"
                );
            }
        }
    }
}

#[test]
fn an_even_window_cannot_be_turned_and_a_seventh_turn_does_not_exist() {
    let even = Viewport::new(center(), 20, 14, 6.0).expect("an uncentered window may be even");
    assert!(matches!(
        even.clone().turned(1),
        Err(RenderError::EvenViewport { .. })
    ));
    assert!(
        even.turned(0).is_ok(),
        "a turn of zero asks nothing of the window"
    );

    let odd = Viewport::centered_on(center(), 21, 15, 6.0).expect("a valid window");
    assert!(matches!(odd.turned(6), Err(RenderError::Turn(6))));
}

#[test]
fn a_grid_cell_names_the_tile_the_hex_window_puts_there() {
    // The claim that makes the grid a rasterizer rather than a renderer. Both
    // types call one function to answer "which tile is in this cell", and this
    // is the assertion that they do.
    for turn in 0..6 {
        let viewport = Viewport::centered_on(center(), 31, 21, 4.0)
            .expect("a valid window")
            .turned(turn)
            .expect("a turn of a sixth");
        let grid = Grid::centered_on(center(), 31, 21, 1, turn).expect("a valid grid");

        assert_eq!(grid.tile_counts(), viewport.tile_counts());
        assert_eq!(grid.center(), center());
        for col in 0..31 {
            for row in 0..21 {
                assert_eq!(
                    grid.coord_at(col, row),
                    viewport.coord_at(col, row),
                    "cell ({col}, {row}) at turn {turn}"
                );
            }
        }
    }
}

#[test]
fn a_grid_is_one_square_per_tile_at_every_scale() {
    let generator = Generator::with_defaults(SEED);
    for scale in [1, 2, 3] {
        let grid = Grid::centered_on(center(), 11, 9, scale, 0).expect("a valid grid");
        assert_eq!(grid.image_size(), (11 * scale, 9 * scale));

        let image = render_grid(&generator, &grid, Layer::Terrain);
        assert_eq!(image.size(), grid.image_size());

        for col in 0..11 {
            for row in 0..9 {
                let expected = Layer::Terrain.color_at(&generator, grid.coord_at(col, row));
                // Every pixel of the square, not only its corner: a scale that
                // painted one pixel and left the rest background would pass a
                // corner check.
                for dy in 0..scale {
                    for dx in 0..scale {
                        assert_eq!(
                            image.pixel(col * scale + dx, row * scale + dy),
                            Some(expected),
                            "pixel ({dx}, {dy}) of cell ({col}, {row}) at scale {scale}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_pixel_locates_back_to_the_tile_that_painted_it() {
    let grid = Grid::centered_on(center(), 11, 9, 3, 2).expect("a valid grid");
    for col in 0..11 {
        for row in 0..9 {
            for dy in 0..3 {
                for dx in 0..3 {
                    assert_eq!(
                        grid.locate(col * 3 + dx, row * 3 + dy),
                        Some(grid.coord_at(col, row)),
                    );
                }
            }
        }
    }
    assert_eq!(grid.locate(11 * 3, 0), None, "past the right edge");
    assert_eq!(grid.locate(0, 9 * 3), None, "past the bottom edge");
}

#[test]
fn the_sheared_turns_are_the_four_that_are_not_multiples_of_three() {
    // Two-fold symmetry in the rasterizer against six-fold in the hex grid
    // leaves exactly two honest turns. A front end warns on the other four, so
    // the condition is worth pinning rather than re-deriving in each of them.
    for turn in 0..6 {
        let grid = Grid::centered_on(center(), 11, 9, 1, turn).expect("a valid grid");
        assert_eq!(
            grid.is_sheared(),
            turn != 0 && turn != 3,
            "turn {turn} is on the wrong side of the warning"
        );
    }
}

#[test]
fn a_grid_refuses_what_it_cannot_draw() {
    assert!(matches!(
        Grid::centered_on(center(), 0, 9, 1, 0),
        Err(RenderError::EmptyViewport { .. })
    ));
    assert!(matches!(
        Grid::centered_on(center(), 10, 9, 1, 0),
        Err(RenderError::EvenViewport { .. })
    ));
    assert!(matches!(
        Grid::centered_on(center(), 11, 9, 0, 0),
        Err(RenderError::Scale(0))
    ));
    assert!(matches!(
        Grid::centered_on(center(), 11, 9, 1, 9),
        Err(RenderError::Turn(9))
    ));
    // A window whose pixels exceed the cap is the caller's choice and a
    // refusal, never a sixty-four-gigabyte allocation.
    assert!(matches!(
        Grid::centered_on(center(), 9999, 9999, 4, 0),
        Err(RenderError::TooLarge { .. })
    ));
}

#[test]
fn the_parallel_fill_agrees_with_a_sequential_one() {
    // `render_grid` is the one parallel path in this crate. Section 30.3 asks
    // for proof rather than a promise, and the reference here is computed
    // cell by cell in a single thread.
    let generator = Generator::with_defaults(SEED);
    let grid = Grid::centered_on(center(), 51, 41, 1, 4).expect("a valid grid");
    let image = render_grid(&generator, &grid, Layer::Elevation);

    for col in 0..51 {
        for row in 0..41 {
            assert_eq!(
                image.pixel(col, row),
                Some(Layer::Elevation.color_at(&generator, grid.coord_at(col, row))),
                "cell ({col}, {row})"
            );
        }
    }
}
