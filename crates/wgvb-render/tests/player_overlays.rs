//! Player overlay composition: `DESIGN.md` sections 27.6 and 29.
//!
//! Decoded RGBA, never PNG file bytes. The `png` crate's default filter and
//! compression settings can change between versions, and a golden that compared
//! files would fail on a dependency bump that changed no pixel.

use wgvb::{Config, Coord, Generator};
use wgvb_render::{
    FOG, Layer, Overlays, SETTLEMENT, SETTLEMENT_EDGE, Viewport, render, render_player,
};

/// The generator every test here renders.
fn generator() -> Generator {
    Generator::new(0x00c0_ffee, Config::default()).expect("the defaults are valid")
}

/// A small window with a center cell.
fn viewport() -> Viewport {
    Viewport::new(Coord::new(-40, 25), 11, 9, 9.0).expect("a valid viewport")
}

#[test]
fn no_overlays_renders_exactly_what_the_plain_renderer_renders() {
    // The composition must be additive. If an empty overlay set moved a single
    // pixel, every diagnostic image and every golden would be describing a
    // different renderer than the player sees.
    let (generator, viewport) = (generator(), viewport());
    for layer in Layer::ALL {
        let plain = render(&generator, &viewport, layer);
        let composed = render_player(&generator, &viewport, layer, &Overlays::none());
        assert_eq!(
            plain.rgba(),
            composed.rgba(),
            "an empty overlay set changed the {} layer",
            layer.name()
        );
    }
}

#[test]
fn an_empty_discovery_list_means_fog_is_off_rather_than_everything_hidden() {
    let overlays = Overlays::none();
    assert!(!overlays.fog_of_war());
    assert!(overlays.is_visible(Coord::new(3, 4)));

    let one = Overlays::new(vec![Coord::new(0, 0)], Vec::new());
    assert!(one.fog_of_war(), "one discovery did not switch fog on");
    assert!(one.is_visible(Coord::new(0, 0)));
    assert!(!one.is_visible(Coord::new(1, 0)));
}

#[test]
fn an_undiscovered_tile_is_drawn_as_fog_and_a_discovered_one_is_not() {
    let (generator, viewport) = (generator(), viewport());

    // Discover exactly one cell of the window, so the image has both cases in
    // it and neither can be produced by a renderer that ignores the overlays.
    let seen = viewport.coord_at(2, 3);
    let unseen = viewport.coord_at(7, 1);
    assert_ne!(seen, unseen);

    let overlays = Overlays::new(vec![seen], Vec::new());
    let plain = render(&generator, &viewport, Layer::Terrain);
    let composed = render_player(&generator, &viewport, Layer::Terrain, &overlays);

    let seen_pixel = viewport.center_pixel(2, 3);
    let unseen_pixel = viewport.center_pixel(7, 1);

    assert_eq!(
        composed.pixel(seen_pixel.0, seen_pixel.1),
        plain.pixel(seen_pixel.0, seen_pixel.1),
        "a discovered tile was not drawn as its terrain"
    );
    assert_eq!(
        composed.pixel(unseen_pixel.0, unseen_pixel.1),
        Some(FOG),
        "an undiscovered tile was not drawn as fog"
    );
}

#[test]
fn fog_hides_every_layer_the_same_way() {
    // Fog is the absence of knowledge, so it cannot depend on which scalar the
    // player happens to be looking at. A fogged tile that leaked its heat band
    // would be a map that tells you the climate of ground you have never seen.
    let (generator, viewport) = (generator(), viewport());
    let overlays = Overlays::new(vec![viewport.coord_at(0, 0)], Vec::new());
    let hidden = viewport.center_pixel(5, 5);

    for layer in Layer::ALL {
        let composed = render_player(&generator, &viewport, layer, &overlays);
        assert_eq!(
            composed.pixel(hidden.0, hidden.1),
            Some(FOG),
            "the {} layer showed through the fog",
            layer.name()
        );
    }
}

#[test]
fn a_settlement_marker_is_drawn_over_its_tile() {
    let (generator, viewport) = (generator(), viewport());
    let coord = viewport.coord_at(5, 4);
    let overlays = Overlays::new(vec![], vec![(coord, "Ashford".to_owned())]);

    let composed = render_player(&generator, &viewport, Layer::Terrain, &overlays);
    let (x, y) = viewport.center_pixel(5, 4);
    assert_eq!(composed.pixel(x, y), Some(SETTLEMENT));

    // The ring. At a hex radius of 9 the marker half-extent is 3, so the
    // eighth pixel out is past the ring and back to terrain.
    let plain = render(&generator, &viewport, Layer::Terrain);
    assert_eq!(composed.pixel(x + 4, y), Some(SETTLEMENT_EDGE));
    assert_eq!(composed.pixel(x + 8, y), plain.pixel(x + 8, y));
}

#[test]
fn a_settlement_is_drawn_through_fog() {
    // Fog hides the world; it does not hide what the player built. A player
    // whose own town vanished from the map would be reading a lie.
    let (generator, viewport) = (generator(), viewport());
    let town = viewport.coord_at(5, 4);
    let overlays = Overlays::new(
        vec![viewport.coord_at(0, 0)],
        vec![(town, "Ashford".to_owned())],
    );

    let composed = render_player(&generator, &viewport, Layer::Terrain, &overlays);
    let (x, y) = viewport.center_pixel(5, 4);
    assert!(!overlays.is_visible(town), "the town's tile was discovered");
    assert_eq!(composed.pixel(x, y), Some(SETTLEMENT));
    // The ground around it is still unknown.
    assert_eq!(composed.pixel(x + 8, y), Some(FOG));
}

#[test]
fn a_marker_at_the_edge_of_the_image_is_clipped_rather_than_wrapped() {
    // The center pixel of an edge cell is inside the image; its marker is not.
    // An unclipped write would land on the opposite side of the row above.
    let generator = generator();
    let viewport = Viewport::new(Coord::new(0, 0), 5, 5, 4.0).expect("a valid viewport");
    let (cols, rows) = viewport.tile_counts();
    let corner = viewport.coord_at(cols - 1, rows - 1);
    let overlays = Overlays::new(vec![], vec![(corner, "Edge".to_owned())]);

    let composed = render_player(&generator, &viewport, Layer::Terrain, &overlays);
    let (width, height) = composed.size();
    let plain = render(&generator, &viewport, Layer::Terrain);

    // Row zero is nowhere near the corner cell, so nothing there may have moved.
    for x in 0..width {
        assert_eq!(
            composed.pixel(x, 0),
            plain.pixel(x, 0),
            "a clipped marker wrote into row 0 at x = {x}"
        );
    }
    assert!(height > 0);
}

#[test]
fn overlapping_markers_resolve_in_coordinate_order() {
    // Two settlements close enough for their markers to touch. Whichever wins
    // must win every time: section 29 asks for a stable render order precisely
    // because overlapping marks otherwise resolve by traversal accident.
    let (generator, viewport) = (generator(), viewport());
    let first = viewport.coord_at(4, 4);
    let second = viewport.coord_at(5, 4);

    let one_order = Overlays::new(
        vec![],
        vec![(first, "A".to_owned()), (second, "B".to_owned())],
    );
    let other_order = Overlays::new(
        vec![],
        vec![(second, "B".to_owned()), (first, "A".to_owned())],
    );
    assert_eq!(
        one_order, other_order,
        "construction order survived sorting"
    );

    let left = render_player(&generator, &viewport, Layer::Terrain, &one_order);
    let right = render_player(&generator, &viewport, Layer::Terrain, &other_order);
    assert_eq!(left.rgba(), right.rgba());
}

#[test]
fn overlays_sort_and_deduplicate_what_they_are_given() {
    let coords = vec![Coord::new(5, 5), Coord::new(-1, 0), Coord::new(5, 5)];
    let overlays = Overlays::new(
        coords,
        vec![
            (Coord::new(2, 2), "Old".to_owned()),
            (Coord::new(0, 0), "Origin".to_owned()),
            (Coord::new(2, 2), "New".to_owned()),
        ],
    );

    assert_eq!(
        overlays.discovered(),
        [Coord::new(-1, 0), Coord::new(5, 5)],
        "discoveries were not sorted and deduplicated"
    );
    assert_eq!(
        overlays.settlements().len(),
        2,
        "one tile kept two settlements"
    );
    assert_eq!(overlays.settlement_at(Coord::new(2, 2)), Some("New"));
    assert_eq!(overlays.settlement_at(Coord::new(0, 0)), Some("Origin"));
    assert_eq!(overlays.settlement_at(Coord::new(9, 9)), None);
}

#[test]
fn composition_is_deterministic_across_repeated_renders() {
    let (generator, viewport) = (generator(), viewport());
    let overlays = Overlays::new(
        (0..20).map(|n| viewport.coord_at(n % 11, n % 9)).collect(),
        vec![
            (viewport.coord_at(1, 1), "One".to_owned()),
            (viewport.coord_at(8, 6), "Two".to_owned()),
        ],
    );

    let first = render_player(&generator, &viewport, Layer::Terrain, &overlays);
    for _ in 0..4 {
        assert_eq!(
            render_player(&generator, &viewport, Layer::Terrain, &overlays).rgba(),
            first.rgba()
        );
    }
}
