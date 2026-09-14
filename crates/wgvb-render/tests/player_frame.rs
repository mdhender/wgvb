//! The player frame: the transform of `DESIGN.md` appendix A, *Coordinate
//! frames*, and the rule that it never reaches the generator.
//!
//! The expected values here are derived from the appendix rather than recorded
//! from the implementation: the direction tables are the ones written out in
//! *Rotation senses*, and the round trips are algebraic identities that hold
//! for reasons stated in the appendix, not for reasons the code happens to
//! have.

use wgvb::{Coord, Generator, Sample, Tile, WORLD_RADIUS};
use wgvb_render::{FrameError, PlayerFrame};

/// **The compile-level half of "a player frame never reaches the generator".**
///
/// Sampling takes a coordinate and nothing else. Adding a rotation, a frame, or
/// a player id to any of these signatures fails to compile here, which is the
/// point: `Tile = F(seed, coordinate, version, configuration)` has no
/// per-player term, and the moment one of these grows a second argument it has
/// one. The other half is structural and needs no test — `wgvb` does not depend
/// on `wgvb-render`, so it cannot name `PlayerFrame` at all.
const _: fn(&Generator, Coord) -> Tile = Generator::tile;
const _: fn(&Generator, Coord) -> Sample = Generator::sample;
const _: fn(&Generator, Coord) -> f64 = Generator::elevation_at;
const _: fn(&Generator, Coord) -> f64 = Generator::relief;

/// Coordinates that exercise the awkward parts: the origin, both signs, and one
/// tile on each of the six wrapped edges.
fn probe_coords() -> Vec<Coord> {
    let mut coords = vec![
        Coord::ORIGIN,
        Coord::new(1, 0),
        Coord::new(-1, 0),
        Coord::new(-7, 12),
        Coord::new(12_345, -30_000),
        Coord::new(-30_000, 12_345),
    ];
    // The midpoint of each of the six edges of the canonical hexagon, reached
    // by rotating the +q edge into place.
    for direction in 0..6 {
        let edge = Coord::new(WORLD_RADIUS, 0).rotate(direction);
        coords.push(edge);
        coords.push(edge.neighbor(direction));
    }
    coords
}

/// Origins that are themselves awkward, paired with every rotation.
fn probe_frames() -> Vec<PlayerFrame> {
    let origins = [
        Coord::ORIGIN,
        Coord::new(3, -1),
        Coord::new(-9, -4),
        Coord::new(WORLD_RADIUS, 0),
        Coord::new(-WORLD_RADIUS, WORLD_RADIUS),
    ];
    let mut frames = Vec::new();
    for origin in origins {
        for rotation in 0..6 {
            frames.push(PlayerFrame::new(origin, rotation).expect("a direction index"));
        }
    }
    frames
}

#[test]
fn the_coordinate_round_trip_is_the_identity_both_ways() {
    for frame in probe_frames() {
        for coord in probe_coords() {
            assert_eq!(
                frame.to_relative(frame.to_absolute(coord)),
                coord,
                "{frame:?} relative {coord:?}"
            );
            assert_eq!(
                frame.to_absolute(frame.to_relative(coord)),
                coord,
                "{frame:?} absolute {coord:?}"
            );
        }
    }
}

#[test]
fn a_frames_own_origin_is_its_own_zero() {
    for frame in probe_frames() {
        assert_eq!(
            frame.to_absolute(Coord::ORIGIN),
            frame.origin(),
            "{frame:?}"
        );
        assert_eq!(
            frame.to_relative(frame.origin()),
            Coord::ORIGIN,
            "{frame:?}"
        );
    }
}

#[test]
fn an_unrotated_frame_at_the_origin_is_the_identity() {
    // The absolute frame is a player frame, and the one every diagnostic render
    // is drawn in.
    let frame = PlayerFrame::new(Coord::ORIGIN, 0).expect("a direction index");
    for coord in probe_coords() {
        assert_eq!(frame.to_absolute(coord), coord, "{coord:?}");
        assert_eq!(frame.to_relative(coord), coord, "{coord:?}");
    }
    for direction in 0..6_u8 {
        assert_eq!(frame.to_absolute_direction(i32::from(direction)), direction);
        assert_eq!(frame.to_relative_direction(i32::from(direction)), direction);
    }
}

#[test]
fn a_step_in_the_players_own_direction_lands_where_the_absolute_step_lands() {
    // This is what ties the two halves of the transform together: converting a
    // coordinate and converting a heading have to agree, or a player walking
    // "north" would arrive somewhere the map does not put them.
    for frame in probe_frames() {
        for coord in probe_coords() {
            for direction in 0..6 {
                let walked = frame.to_absolute(coord.neighbor(direction));
                let stepped = frame
                    .to_absolute(coord)
                    .neighbor(i32::from(frame.to_absolute_direction(direction)));
                assert_eq!(walked, stepped, "{frame:?} at {coord:?} facing {direction}");
            }
        }
    }
}

#[test]
fn direction_conversion_round_trips_for_all_thirty_six_combinations() {
    for rotation in 0..6 {
        let frame = PlayerFrame::new(Coord::new(5, -2), rotation).expect("a direction index");
        for direction in 0..6 {
            let absolute = frame.to_absolute_direction(direction);
            assert!(absolute < 6, "rotation {rotation} direction {direction}");
            assert_eq!(
                i32::from(absolute),
                (direction + i32::from(rotation)).rem_euclid(6),
                "rotation {rotation} direction {direction}"
            );
            assert_eq!(
                i32::from(frame.to_relative_direction(i32::from(absolute))),
                direction,
                "rotation {rotation} direction {direction}"
            );
        }
    }
}

#[test]
fn direction_conversion_accepts_any_integer() {
    let frame = PlayerFrame::new(Coord::new(-4, 8), 4).expect("a direction index");
    for direction in -13..13 {
        assert_eq!(
            frame.to_absolute_direction(direction),
            frame.to_absolute_direction(direction + 6),
            "direction {direction}"
        );
        assert_eq!(
            frame.to_relative_direction(direction),
            frame.to_relative_direction(direction + 6),
            "direction {direction}"
        );
    }
    // The extremes normalize rather than overflow the sum with the rotation.
    assert!(frame.to_absolute_direction(i32::MAX) < 6);
    assert!(frame.to_absolute_direction(i32::MIN) < 6);
    assert!(frame.to_relative_direction(i32::MAX) < 6);
    assert!(frame.to_relative_direction(i32::MIN) < 6);
}

#[test]
fn two_players_agree_on_the_tile_and_disagree_on_the_heading() {
    // Ashe and Bram are looking at the same tile and describing it to each
    // other. Neither one's numbers mean anything to the other.
    let ashe = PlayerFrame::new(Coord::new(100, -40), 1).expect("a direction index");
    let bram = PlayerFrame::new(Coord::new(-7, 900), 4).expect("a direction index");

    let tile = Coord::new(-12_000, 5_000);
    let ashe_says = ashe.to_relative(tile);
    let bram_says = bram.to_relative(tile);

    assert_ne!(ashe_says, bram_says, "two frames named it the same");
    assert_eq!(ashe.to_absolute(ashe_says), tile);
    assert_eq!(bram.to_absolute(bram_says), tile);
    // And each one's coordinate is nonsense in the other's frame.
    assert_ne!(bram.to_absolute(ashe_says), tile);

    // One shared heading — due absolute 3 — has a different number for each.
    let heading = 3;
    assert_eq!(ashe.to_relative_direction(heading), 2);
    assert_eq!(bram.to_relative_direction(heading), 5);
    assert_ne!(
        ashe.to_relative_direction(heading),
        bram.to_relative_direction(heading)
    );
    // Their norths are their own direction 0, and they are not the same north.
    assert_eq!(ashe.to_absolute_direction(0), 1);
    assert_eq!(bram.to_absolute_direction(0), 4);
}

#[test]
fn the_compass_walk_is_the_appendix_table_for_every_rotation() {
    // `DESIGN.md` appendix A, *Rotation senses*. Walking the compass clockwise
    // from the viewer's own north is 0, 5, 4, 3, 2, 1 in that viewer's
    // numbering — the same six numbers for every viewer, which is the point —
    // and k, k-1, k-2, k-3, k-4, k-5 in absolute ones.
    const COMPASS: [&str; 6] = ["N", "NE", "SE", "S", "SW", "NW"];
    const RELATIVE: [u8; 6] = [0, 5, 4, 3, 2, 1];

    for rotation in 0..6 {
        let frame = PlayerFrame::new(Coord::new(-1, 1), rotation).expect("a direction index");
        for (step, (point, relative)) in COMPASS.iter().zip(RELATIVE).enumerate() {
            let step = i32::try_from(step).expect("six fits an i32");
            let expected = (i32::from(rotation) - step).rem_euclid(6);
            assert_eq!(
                i32::from(frame.to_absolute_direction(i32::from(relative))),
                expected,
                "rotation {rotation} {point}"
            );
        }
        // Whatever a viewer's north is, south is their own 3.
        assert_eq!(
            frame.to_relative_direction(i32::from(frame.to_absolute_direction(0)) + 3),
            3,
            "rotation {rotation}"
        );
    }
}

#[test]
fn the_admin_frame_is_rotation_two() {
    // The diagnostic renderer applies no rotation and its flat-top layout puts
    // absolute direction 2 at the top of the image, so what it draws is a
    // viewer at k = 2. Appendix A's table for that viewer, absolute:
    //   N 2, NE 1, SE 0, S 5, SW 4, NW 3.
    let admin = PlayerFrame::new(Coord::ORIGIN, 2).expect("a direction index");
    let clockwise_from_north: [u8; 6] = [0, 5, 4, 3, 2, 1];
    let absolute: [u8; 6] = [2, 1, 0, 5, 4, 3];
    for (relative, expected) in clockwise_from_north.into_iter().zip(absolute) {
        assert_eq!(
            admin.to_absolute_direction(i32::from(relative)),
            expected,
            "relative {relative}"
        );
        assert_eq!(
            admin.to_relative_direction(i32::from(expected)),
            relative,
            "absolute {expected}"
        );
    }
}

#[test]
fn an_out_of_range_rotation_is_refused_rather_than_reduced() {
    for rotation in [6_u8, 7, 12, 255] {
        assert_eq!(
            PlayerFrame::new(Coord::ORIGIN, rotation),
            Err(FrameError::Rotation(rotation)),
            "rotation {rotation}"
        );
    }
    for rotation in 0..6 {
        assert!(PlayerFrame::new(Coord::ORIGIN, rotation).is_ok());
    }
}
