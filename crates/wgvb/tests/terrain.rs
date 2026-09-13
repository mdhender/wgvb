//! Terrain classification, over large samples.
//!
//! `DESIGN.md` sections 15, 17, 30.2, 30.3, 30.4, 30.5, 30.6, 30.7, 30.8,
//! 30.10, 30.12, 33.1, and 33.4.
//!
//! # What these tests are for
//!
//! The phase 6 exit condition is a judgement — *terrain maps visually
//! correspond to elevation and climate, and boundaries look geographically
//! plausible* — and no assertion settles it. The renders in `docs/renders/v5`
//! are the evidence for that half. What these tests can do is rule out the
//! ways a classifier can be wrong while still producing a picture:
//!
//! - **Section 33.1's defect.** `hash(seed, q, r) % TERRAIN_COUNT` produces a
//!   perfect distribution and no geography at all, so a distribution test
//!   cannot catch it and a neighbor test can. Agreement between neighbors is
//!   the measurement that noise cannot fake.
//! - **A vocabulary that is not used.** A terrain no world ever contains is a
//!   rule that never fires, and one that holds most of the map is a rule that
//!   swallowed the others.
//! - **The rule order.** Section 17 orders the rules so that exceptional
//!   terrain is not hidden by a broad biome rule. A wrong order still produces
//!   a plausible map — one with no volcanoes in it.
//! - **The two terrains that must never appear.** Inland water is omitted in
//!   this version, and the omission is a decision rather than an oversight, so
//!   it is asserted rather than assumed. See the decision record in the
//!   `terrain` module.
//!
//! Nothing here derives a threshold from the sample. Section 33.5: the
//! configuration says where the rules are, and the sample only says what that
//! produces.

use wgvb::{Climate, Config, Coord, Elevation, Generator, HeatBand, MoistureBand, Terrain, Tile};

/// Seeds every distribution test runs over, so a bound that only one world
/// satisfies fails rather than passing on the lucky seed.
const SEEDS: [u64; 4] = [1, 0xfeed_face, 0x0123_4567_89ab_cdef, 42];

/// A wide, decorrelated spread of coordinates.
///
/// The strides are well above the longest feature terrain reads, so
/// consecutive samples are independent draws rather than a walk across one
/// region. Negative on both axes.
fn spread() -> Vec<Coord> {
    let mut out = Vec::new();
    for i in -90..90_i64 {
        for j in -90..90_i64 {
            out.push(Coord::new(i * 37 + 11, j * 41 - 7));
        }
    }
    out
}

/// A contiguous block of tiles, for the tests that care about neighbors.
fn block(origin: (i64, i64), edge: i64) -> Vec<Coord> {
    let mut out = Vec::new();
    for q in 0..edge {
        for r in 0..edge {
            out.push(Coord::new(origin.0 + q, origin.1 + r));
        }
    }
    out
}

/// Four widely separated contiguous blocks.
///
/// One block can sit entirely inside one climate zone and one elevation band,
/// which is the right answer for that place and a poor sample of a world.
const WINDOWS: [(i64, i64); 4] = [
    (-600, -450),
    (11_000, -4_500),
    (-7_000, 2_200),
    (2_500, 9_100),
];

/// Terrain occupancy of a sample, indexed by discriminant.
fn counts(tiles: &[Tile]) -> [u32; 27] {
    let mut counts = [0_u32; 27];
    for tile in tiles {
        counts[tile.terrain as usize] += 1;
    }
    counts
}

#[test]
fn no_tile_is_ever_inland_water() {
    // The decision record, as a test. Section 17 permits inland water only
    // where bounded local generation can give it coherent membership, surface
    // elevation, depth, and shorelines, and requires it to be *omitted* rather
    // than approximated otherwise. This version omits it, and this is where
    // that claim is kept honest: if a later phase ships lakes, this test is
    // the one that has to be deliberately rewritten, which is exactly the
    // amount of friction the decision deserves.
    //
    // The `terrain` module has the reasoning and the two approximations that
    // were rejected. The short form: one lake has one surface elevation, and
    // knowing which tiles share it is connectivity.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        for coord in spread() {
            let terrain = generator.tile(coord).terrain;
            assert!(
                !terrain.is_inland_water(),
                "seed {seed:#x} at {coord:?} is {}",
                terrain.name()
            );
        }
        for origin in WINDOWS {
            for tile in generator.tiles(&block(origin, 64)) {
                assert!(
                    !tile.terrain.is_inland_water(),
                    "seed {seed:#x} at {:?} is {}",
                    tile.coord,
                    tile.terrain.name()
                );
            }
        }
    }
}

#[test]
fn every_terrain_this_version_generates_is_reachable() {
    // A rule that never fires is a rule nobody has tested. Pooled across the
    // four seeds rather than per seed: a world with no rainforest anywhere is
    // a possible world, and a *vocabulary* with no rainforest in it is a bug.
    let mut seen = [0_u32; 27];
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        for (index, count) in counts(&generator.tiles(&spread())).into_iter().enumerate() {
            seen[index] += count;
        }
        for origin in WINDOWS {
            for (index, count) in counts(&generator.tiles(&block(origin, 96)))
                .into_iter()
                .enumerate()
            {
                seen[index] += count;
            }
        }
    }
    for terrain in Terrain::ALL {
        let count = seen[terrain as usize];
        if terrain.is_inland_water() {
            assert_eq!(
                count,
                0,
                "{} is not generated in this version",
                terrain.name()
            );
        } else {
            assert!(count > 0, "{} never occurs", terrain.name());
        }
    }
}

#[test]
fn no_terrain_holds_the_world_and_volcanoes_are_rare() {
    // The other half of a distribution test. Deep ocean is allowed to be the
    // commonest thing on a world that is two thirds water; nothing on land is.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let tiles = generator.tiles(&spread());
        let counts = counts(&tiles);
        let total = f64::from(u32::try_from(tiles.len()).expect("a test sample fits in u32"));
        let land = f64::from(
            u32::try_from(tiles.iter().filter(|t| t.elevation.is_land()).count())
                .expect("a test sample fits in u32"),
        );

        for terrain in Terrain::ALL {
            let share = f64::from(counts[terrain as usize]) / total;
            let limit = if terrain == Terrain::DeepOcean {
                0.70
            } else {
                0.25
            };
            assert!(
                share <= limit,
                "seed {seed:#x}: {} holds {share:.4} of the world",
                terrain.name()
            );
        }

        // Volcanoes are rare by construction — restless crust, uplift, and a
        // local peak, all at once — and the bound is on the conjunction rather
        // than on any one of them. A volcano share anywhere near a per-tile
        // draw's would mean one of the three conditions had stopped biting.
        let volcanoes = f64::from(counts[Terrain::Volcano as usize]) / land;
        assert!(
            volcanoes < 0.01,
            "seed {seed:#x}: volcanoes are {volcanoes:.5} of the land"
        );
        let volcanic = f64::from(counts[Terrain::VolcanicHighland as usize]) / land;
        assert!(
            volcanic < 0.06,
            "seed {seed:#x}: volcanic highland is {volcanic:.5} of the land"
        );
    }
}

#[test]
fn neighboring_tiles_usually_share_a_terrain() {
    // Section 30.7, and the measurement section 33.1's defect cannot pass. An
    // independent per-tile draw over twenty-five reachable terrains would
    // agree with a neighbor about four per cent of the time; a classification
    // of continuous fields agrees most of the time, and disagrees along lines
    // rather than at points.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut agreeing = 0_u32;
        let mut pairs = 0_u32;
        let mut isolated = 0_u32;
        let mut tiles = 0_u32;
        for origin in WINDOWS {
            for coord in block(origin, 64) {
                let here = generator.tile(coord).terrain;
                let mut differing = 0_u32;
                for direction in 0..6 {
                    let there = generator.tile(coord.neighbor(direction)).terrain;
                    agreeing += u32::from(here == there);
                    differing += u32::from(here != there);
                    pairs += 1;
                }
                isolated += u32::from(differing == 6);
                tiles += 1;
            }
        }
        let agreement = f64::from(agreeing) / f64::from(pairs);
        assert!(
            agreement > 0.6,
            "seed {seed:#x}: only {agreement:.3} of neighbor pairs share a terrain"
        );
        // And a tile that shares nothing with any of its six neighbors is the
        // shape speckle takes. A few are real — a volcano is one by
        // definition, and so is a lone tile on a threshold — but they must be
        // the exception rather than the texture.
        let alone = f64::from(isolated) / f64::from(tiles);
        assert!(
            alone < 0.05,
            "seed {seed:#x}: {alone:.4} of tiles share no terrain with any neighbor"
        );
    }
}

#[test]
fn coast_and_coastal_water_only_occur_at_the_water_s_edge() {
    // Section 17's definition of both, checked against the neighbors rather
    // than against the rule that produced them. A coast in the middle of a
    // continent, or coastal water in the middle of an ocean, would mean the
    // adjacency had been read from the wrong array.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut coasts = 0_u32;
        let mut coastal_water = 0_u32;
        for origin in WINDOWS {
            for coord in block(origin, 64) {
                let here = generator.tile(coord);
                let mut water_neighbor = false;
                let mut land_neighbor = false;
                for direction in 0..6 {
                    if generator
                        .tile(coord.neighbor(direction))
                        .elevation
                        .is_water()
                    {
                        water_neighbor = true;
                    } else {
                        land_neighbor = true;
                    }
                }
                match here.terrain {
                    Terrain::Coast => {
                        assert!(
                            water_neighbor,
                            "seed {seed:#x}: a coast at {coord:?} with no water beside it"
                        );
                        assert!(here.elevation.is_land(), "a coast is land");
                        coasts += 1;
                    }
                    Terrain::CoastalWater => {
                        assert!(
                            land_neighbor,
                            "seed {seed:#x}: coastal water at {coord:?} with no land beside it"
                        );
                        assert!(here.elevation.is_water(), "coastal water is water");
                        coastal_water += 1;
                    }
                    _ => {}
                }
            }
        }
        assert!(
            coasts > 100 && coastal_water > 100,
            "seed {seed:#x}: too few shoreline tiles to have tested anything: \
             {coasts} coast, {coastal_water} coastal water"
        );
    }
}

#[test]
fn open_water_is_never_next_to_open_land_without_a_shoreline_between_them() {
    // The other side of the same rule, and the one a reader of a map would
    // notice: every water tile touching land is coastal water, so the ocean
    // family always meets the land through its own shore. Deep ocean directly
    // against a forest would be a classifier that had forgotten the
    // adjacency.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        for origin in WINDOWS {
            for coord in block(origin, 48) {
                let here = generator.tile(coord);
                if !here.terrain.is_water() || here.terrain == Terrain::CoastalWater {
                    continue;
                }
                for direction in 0..6 {
                    let there = generator.tile(coord.neighbor(direction));
                    assert!(
                        there.terrain.is_water(),
                        "seed {seed:#x}: {} at {coord:?} touches {} at {:?}",
                        here.terrain.name(),
                        there.terrain.name(),
                        there.coord
                    );
                }
            }
        }
    }
}

#[test]
fn terrain_corresponds_to_the_elevation_band_it_was_classified_from() {
    // The exit condition's first half, as far as an assertion can take it. A
    // reader comparing the terrain and elevation layers of one window has to
    // see the same shapes, and the strongest form of that is the partition
    // both agree on: water is water.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        for coord in spread() {
            let tile = generator.tile(coord);
            assert_eq!(
                tile.terrain.is_water(),
                tile.elevation.is_water(),
                "seed {seed:#x} at {coord:?}: {} on {:?}",
                tile.terrain.name(),
                tile.elevation
            );
            // And the elevated family only ever sits on elevated ground.
            if matches!(
                tile.terrain,
                Terrain::Mountain | Terrain::Alpine | Terrain::Hills
            ) {
                assert!(
                    tile.elevation >= Elevation::Upland,
                    "seed {seed:#x} at {coord:?}: {} on {:?}",
                    tile.terrain.name(),
                    tile.elevation
                );
            }
            // Wetlands and coasts are lowland by rule, and a rule that had
            // moved would put a marsh on a mountain.
            if matches!(
                tile.terrain,
                Terrain::Marsh | Terrain::Swamp | Terrain::Bog | Terrain::Coast
            ) {
                assert_eq!(
                    tile.elevation,
                    Elevation::Lowland,
                    "seed {seed:#x} at {coord:?}: {}",
                    tile.terrain.name()
                );
            }
        }
    }
}

#[test]
fn terrain_corresponds_to_the_climate_it_was_classified_from() {
    // The exit condition's other half. Not every terrain names a climate —
    // hills and mountains are shapes rather than covers — but the ones that do
    // must not contradict the climate layer beside them, or the two images are
    // telling different stories about one world.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        for coord in spread() {
            let tile = generator.tile(coord);
            match tile.terrain {
                Terrain::Rainforest => assert!(
                    tile.climate.heat >= HeatBand::Warm,
                    "seed {seed:#x} at {coord:?}: a rainforest in a {:?} band",
                    tile.climate.heat
                ),
                Terrain::Desert => assert!(
                    tile.climate.heat >= HeatBand::Warm,
                    "seed {seed:#x} at {coord:?}: a desert in a {:?} band",
                    tile.climate.heat
                ),
                Terrain::BorealForest => assert_eq!(
                    tile.climate.heat,
                    HeatBand::Cold,
                    "seed {seed:#x} at {coord:?}",
                ),
                Terrain::Tundra => assert!(
                    tile.climate.heat <= HeatBand::Cold,
                    "seed {seed:#x} at {coord:?}: tundra in a {:?} band",
                    tile.climate.heat
                ),
                Terrain::GlacialIce => assert_eq!(
                    tile.climate.heat,
                    HeatBand::Polar,
                    "seed {seed:#x} at {coord:?}: ice outside the polar band",
                ),
                Terrain::Bog => assert!(
                    tile.climate.heat <= HeatBand::Cold,
                    "seed {seed:#x} at {coord:?}: a bog in a {:?} band",
                    tile.climate.heat
                ),
                Terrain::Swamp => assert!(
                    tile.climate.heat >= HeatBand::Warm,
                    "seed {seed:#x} at {coord:?}: a swamp in a {:?} band",
                    tile.climate.heat
                ),
                _ => {}
            }
        }
    }
}

#[test]
fn a_wetland_is_wetter_and_flatter_than_the_ground_around_it() {
    // The wetland rule's two conditions, measured against the population
    // rather than against the thresholds, so the claim survives a retune of
    // either one. Section 17: wetlands are saturated, low, and flat.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut wetland = (0.0_f64, 0.0_f64, 0_u32);
        let mut lowland = (0.0_f64, 0.0_f64, 0_u32);
        for coord in spread() {
            let tile = generator.tile(coord);
            if tile.elevation != Elevation::Lowland {
                continue;
            }
            let bucket = if matches!(tile.terrain, Terrain::Marsh | Terrain::Swamp | Terrain::Bog) {
                &mut wetland
            } else {
                &mut lowland
            };
            bucket.0 += tile.moisture_value;
            bucket.1 += tile.relief_value;
            bucket.2 += 1;
        }
        assert!(
            wetland.2 > 20 && lowland.2 > 500,
            "seed {seed:#x}: {} wetland against {} other lowland",
            wetland.2,
            lowland.2
        );
        assert!(
            wetland.0 / f64::from(wetland.2) > lowland.0 / f64::from(lowland.2),
            "seed {seed:#x}: wetlands are not wetter than the lowland around them"
        );
        assert!(
            wetland.1 / f64::from(wetland.2) < lowland.1 / f64::from(lowland.2),
            "seed {seed:#x}: wetlands are not flatter than the lowland around them"
        );
    }
}

#[test]
fn the_basin_is_what_puts_the_extra_wetlands_where_they_are() {
    // The attribution the population test above cannot make. Basin influence
    // reaches a wetland only through the product in terrain wetness, so with
    // the coupling disabled the wetlands are a different set — and every tile
    // that becomes a wetland *because* of the coupling has to be in a basin,
    // while every tile that stops being one has to be on a rise. Anything
    // else would mean the sign of the product was wrong.
    let coupled = Generator::with_defaults(0x3d9e_0001);
    let flat = Generator::new(
        0x3d9e_0001,
        Config {
            terrain_basin_weight: 0.0,
            ..Config::default()
        },
    )
    .expect("configuration is valid");

    let wet = [Terrain::Marsh, Terrain::Swamp, Terrain::Bog];
    let mut gained = 0_u32;
    let mut lost = 0_u32;
    for coord in spread() {
        let a = coupled.tile(coord).terrain;
        let b = flat.tile(coord).terrain;
        let basin = coupled.sample(coord).basin_influence;
        if wet.contains(&a) && !wet.contains(&b) {
            gained += 1;
            assert!(
                basin > 0.0,
                "the coupling made a wetland at {coord:?} on a rise ({basin})"
            );
        }
        if !wet.contains(&a) && wet.contains(&b) {
            lost += 1;
            assert!(
                basin < 0.0,
                "the coupling drained a wetland at {coord:?} in a basin ({basin})"
            );
        }
    }
    assert!(
        gained > 0 && lost > 0,
        "the coupling changed nothing: {gained} gained, {lost} lost"
    );
}

#[test]
fn a_dry_basin_is_drier_than_a_dry_slope() {
    // Section 17's endorheic result, measured. The basin term multiplies
    // moisture rather than adding to it, so in a dry climate a closed basin
    // reads as *drier* than open ground of the same climate. With the coupling
    // disabled the two populations are the same world sampled twice.
    let coords = spread();
    let coupled = Generator::with_defaults(0x0006_a510);
    let flat = Generator::new(
        0x0006_a510,
        Config {
            terrain_basin_weight: 0.0,
            ..Config::default()
        },
    )
    .expect("configuration is valid");

    let mut dry_in_basin = 0_u32;
    let mut dry_in_basin_uncoupled = 0_u32;
    let mut wet_in_basin = 0_u32;
    let mut wet_in_basin_uncoupled = 0_u32;
    let mut tiles = 0_u32;
    for coord in &coords {
        let a = coupled.tile(*coord);
        let b = flat.tile(*coord);
        // Everything the coupling must not reach.
        assert_eq!(
            a.elevation_value.to_bits(),
            b.elevation_value.to_bits(),
            "the terrain basin weight reached elevation at {coord:?}"
        );
        assert_eq!(a.climate, b.climate, "{coord:?}");

        if a.elevation.is_water() || coupled.sample(*coord).basin_influence < 0.2 {
            continue;
        }
        tiles += 1;
        let dry = [Terrain::Desert, Terrain::Badlands, Terrain::Scrubland];
        let wet = [Terrain::Marsh, Terrain::Swamp, Terrain::Bog];
        dry_in_basin += u32::from(a.moisture_value < 0.0 && dry.contains(&a.terrain));
        dry_in_basin_uncoupled += u32::from(b.moisture_value < 0.0 && dry.contains(&b.terrain));
        wet_in_basin += u32::from(a.moisture_value > 0.0 && wet.contains(&a.terrain));
        wet_in_basin_uncoupled += u32::from(b.moisture_value > 0.0 && wet.contains(&b.terrain));
    }

    assert!(tiles > 500, "too few basin tiles to measure: {tiles}");
    assert!(
        dry_in_basin > dry_in_basin_uncoupled,
        "the basin coupling dries nothing: {dry_in_basin} against {dry_in_basin_uncoupled}"
    );
    assert!(
        wet_in_basin > wet_in_basin_uncoupled,
        "the basin coupling wets nothing: {wet_in_basin} against {wet_in_basin_uncoupled}"
    );
}

#[test]
fn a_volcano_stands_above_its_neighbors() {
    // The condition that makes a volcano a place rather than a value.
    // Re-derived here from the elevation field rather than read back from the
    // classifier: a volcano is a strict local maximum of the same elevation
    // scalar every other rule reads.
    let mut volcanoes = 0_u32;
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        for origin in WINDOWS {
            for coord in block(origin, 96) {
                let tile = generator.tile(coord);
                if tile.terrain != Terrain::Volcano {
                    continue;
                }
                volcanoes += 1;
                assert!(
                    tile.elevation >= Elevation::Upland,
                    "seed {seed:#x}: a volcano at {coord:?} on {:?}",
                    tile.elevation
                );
                for direction in 0..6 {
                    assert!(
                        generator.elevation_at(coord.neighbor(direction)) < tile.elevation_value,
                        "seed {seed:#x}: the volcano at {coord:?} is not a local maximum"
                    );
                }
                // And no two volcanoes are adjacent, which follows from being
                // a strict local maximum but is the thing a reader of a map
                // would notice if it stopped holding.
                for direction in 0..6 {
                    assert_ne!(
                        generator.tile(coord.neighbor(direction)).terrain,
                        Terrain::Volcano,
                        "seed {seed:#x}: two volcanoes touch at {coord:?}"
                    );
                }
            }
        }
    }
    assert!(volcanoes > 0, "no volcano occurred anywhere to be tested");
}

#[test]
fn the_exceptional_rules_are_not_swallowed_by_the_climate_cover() {
    // Section 17's ordering paragraph, as an experiment on whole worlds rather
    // than on constructed inputs — `terrain.rs`'s unit tests do the latter.
    // Each of the four exceptional rules has to produce terrain that the
    // climate cover alone never could, in a world where the cover would
    // otherwise have claimed those tiles.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let tiles = generator.tiles(&spread());
        let counts = counts(&tiles);

        for terrain in [
            Terrain::GlacialIce,
            Terrain::Volcano,
            Terrain::Mountain,
            Terrain::Alpine,
        ] {
            assert!(
                counts[terrain as usize] > 0,
                "seed {seed:#x}: {} never survives the rules above it",
                terrain.name()
            );
        }

        // Glacial ice appears where the climate cover would have said tundra,
        // so the polar band has to contain both.
        let polar: Vec<&Tile> = tiles
            .iter()
            .filter(|t| t.climate.heat == HeatBand::Polar && t.elevation.is_land())
            .collect();
        assert!(
            polar.iter().any(|t| t.terrain == Terrain::GlacialIce),
            "seed {seed:#x}: nothing in the polar band is ice"
        );
        assert!(
            polar.iter().any(|t| t.terrain != Terrain::GlacialIce),
            "seed {seed:#x}: everything in the polar band is ice"
        );
    }
}

#[test]
fn terrain_is_deterministic_and_order_independent() {
    // Sections 2.1 and 30.2. Forwards, backwards, and interleaved must agree
    // exactly. Terrain reads seven coordinates rather than one, so this is
    // also the check that the neighborhood is a function of the tile rather
    // than of what was generated before it.
    let generator = Generator::with_defaults(0x07e8_8a10);
    let coords = spread();

    let forward: Vec<Tile> = coords.iter().map(|c| generator.tile(*c)).collect();
    let mut backward: Vec<Tile> = coords.iter().rev().map(|c| generator.tile(*c)).collect();
    backward.reverse();
    assert_eq!(forward, backward);

    let mut interleaved = vec![forward[0]; coords.len()];
    for index in (0..coords.len()).step_by(2) {
        interleaved[index] = generator.tile(coords[index]);
    }
    for index in (1..coords.len()).step_by(2) {
        interleaved[index] = generator.tile(coords[index]);
    }
    assert_eq!(interleaved, forward);

    // And the batch forms agree with the single one, per section 20.
    let batched = generator.tiles(&coords);
    assert_eq!(batched, forward);
    let mut into = vec![forward[0]; coords.len()];
    generator.tiles_into(&coords, &mut into);
    assert_eq!(into, forward);
}

#[test]
fn concurrent_generation_produces_identical_terrain() {
    // Section 30.3. Compiles only if `Generator: Sync`, and passes only if no
    // thread schedule can reach a classification.
    let generator = Generator::with_defaults(0x07e8_8a11);
    let g = &generator;
    let coords = spread();
    let expected: Vec<Terrain> = coords.iter().map(|c| g.tile(*c).terrain).collect();

    std::thread::scope(|scope| {
        for worker in 0..8_usize {
            let coords = &coords;
            let expected = &expected;
            scope.spawn(move || {
                for step in 0..coords.len() {
                    let index = (step + worker * 137) % coords.len();
                    assert_eq!(g.tile(coords[index]).terrain, expected[index]);
                }
            });
        }
    });
}

#[test]
fn negative_coordinates_are_no_different_from_positive_ones() {
    // Section 30.4. The classic defect is a distribution that changes with the
    // sign of a coordinate because a bare `/` mirrored an addressing lattice
    // about the origin. Compared as a vocabulary rather than as a histogram:
    // four quadrants of one world genuinely do differ, and what they must not
    // do is differ in what they can contain.
    let generator = Generator::with_defaults(0x07e8_8a12);
    let mut quadrants = Vec::new();
    for quadrant in [(1_i64, 1_i64), (-1, 1), (1, -1), (-1, -1)] {
        let mut seen: Vec<Terrain> = Vec::new();
        let mut land = 0_u32;
        for i in 0..90_i64 {
            for j in 0..90_i64 {
                let tile = generator.tile(Coord::new(
                    quadrant.0 * (i * 53 + 17),
                    quadrant.1 * (j * 59 + 23),
                ));
                if tile.elevation.is_land() {
                    land += 1;
                }
                if !seen.contains(&tile.terrain) {
                    seen.push(tile.terrain);
                }
            }
        }
        assert!(land > 500, "quadrant {quadrant:?} has {land} land tiles");
        quadrants.push(seen.len());
    }
    let lowest = *quadrants.iter().min().expect("four quadrants");
    assert!(
        lowest >= 10,
        "a quadrant produced only {lowest} distinct terrains: {quadrants:?}"
    );
}

#[test]
fn a_wrapped_coordinate_produces_exactly_the_same_terrain() {
    // Sections 7.1 and 30.10, on all six edges. A coordinate and its wrapped
    // image name one tile, so the classification must be identical — including
    // the neighborhood it reads, because `Coord::neighbor` normalizes too.
    let generator = Generator::with_defaults(0x07e8_8a13);
    let n = i64::from(i16::MAX);
    let mirrors = [
        (2 * n + 1, -n),
        (n + 1, -(2 * n + 1)),
        (-n, -(n + 1)),
        (-(2 * n + 1), n),
        (-(n + 1), 2 * n + 1),
        (n, n + 1),
    ];
    for (q, r) in [
        (0_i64, 0_i64),
        (5, -3),
        (-11, 400),
        (n, -n),
        (-n, n),
        (0, n),
        (n, 0),
        (0, -n),
    ] {
        let canonical = Coord::new(q, r);
        let tile = generator.tile(canonical);
        for (mq, mr) in mirrors {
            let wrapped = Coord::new(q + mq, r + mr);
            assert_eq!(wrapped, canonical, "({q}, {r}) + ({mq}, {mr})");
            assert_eq!(
                generator.tile(wrapped),
                tile,
                "({q}, {r}) across ({mq}, {mr})"
            );
        }
    }
}

#[test]
fn crossing_a_chunk_or_region_boundary_is_not_a_terrain_boundary() {
    // Sections 30.5 and 30.6, on the classification rather than on the
    // scalars: `tests/continuity.rs` measures the fields, and this measures
    // what a reader of a map would see. If the addressing lattice were
    // visible, neighbor pairs that straddle a cell edge would disagree more
    // often than the pairs around them.
    let config = Config::default();
    let generator = Generator::new(0x07e8_8a14, config.clone()).expect("configuration is valid");
    for (label, size) in [
        ("chunk", i64::from(config.chunk_size_hexes)),
        ("region", i64::from(config.region_size_hexes)),
        ("macro region", i64::from(config.macro_region_size_hexes)),
    ] {
        let mut boundary = (0_u32, 0_u32);
        let mut interior = (0_u32, 0_u32);
        for fixed in [0_i64, -97, 411, -1_234] {
            for step in -2_048..2_048_i64 {
                let here = generator.tile(Coord::new(step, fixed)).terrain;
                let next = generator.tile(Coord::new(step + 1, fixed)).terrain;
                let crosses = (step + 1).rem_euclid(size) == 0;
                let bucket = if crosses {
                    &mut boundary
                } else {
                    &mut interior
                };
                bucket.0 += u32::from(here != next);
                bucket.1 += 1;
            }
        }
        assert!(boundary.1 >= 24, "{label}: too few crossings to measure");
        let at_edge = f64::from(boundary.0) / f64::from(boundary.1);
        let elsewhere = f64::from(interior.0) / f64::from(interior.1);
        assert!(
            at_edge < elsewhere * 2.0 + 0.02,
            "{label}: terrain changes at {at_edge:.4} of cell edges against \
             {elsewhere:.4} elsewhere"
        );
    }
}

#[test]
fn the_classification_is_total_over_every_band_pair() {
    // Totality is a compile-time property of the `match` in the classifier —
    // that is most of why these are enums — and what a test can add is that
    // every pair is actually reachable through the public API. A pair no
    // generator ever produces would leave an arm of that `match` unexercised.
    let generator = Generator::with_defaults(0x0123_4567_89ab_cdef);
    let mut seen: Vec<(HeatBand, MoistureBand)> = Vec::new();
    for coord in spread() {
        let tile = generator.tile(coord);
        let pair = (tile.climate.heat, tile.climate.moisture);
        if !seen.contains(&pair) {
            seen.push(pair);
        }
    }
    assert_eq!(
        seen.len(),
        25,
        "only {} of the 25 climate pairs occur, so some cover arms are untested",
        seen.len()
    );

    // And the pair alone does not decide the terrain: elevation, relief, and
    // the basin all reach the answer, so one climate has to produce more than
    // one terrain somewhere in the world.
    let mut terrains: Vec<Terrain> = Vec::new();
    let target = Climate {
        heat: HeatBand::Temperate,
        moisture: MoistureBand::Moderate,
    };
    for coord in spread() {
        let tile = generator.tile(coord);
        if tile.climate == target && !terrains.contains(&tile.terrain) {
            terrains.push(tile.terrain);
        }
    }
    assert!(
        terrains.len() > 3,
        "one climate produced only {} terrains, so terrain is climate by another name",
        terrains.len()
    );
}

#[test]
fn a_tile_is_copy_and_small_enough_for_an_allocation_free_batch() {
    // Section 4.1 and section 30.12, asserted from outside the crate as well
    // as inside it: the promise is part of the public API, so a caller has to
    // be able to rely on it.
    assert!(
        size_of::<Tile>() <= 64,
        "Tile is {} bytes",
        size_of::<Tile>()
    );

    let generator = Generator::with_defaults(3);
    let tile = generator.tile(Coord::new(-3, 9));
    let copied = tile;
    assert_eq!(tile, copied);
    assert_eq!(copied.terrain, tile.terrain);

    // And a batch fill really does write into a caller's buffer.
    let coords = [Coord::new(0, 0), Coord::new(1, -1), Coord::new(-5, 12)];
    let mut out = [tile; 3];
    generator.tiles_into(&coords, &mut out);
    for (coord, filled) in coords.into_iter().zip(out) {
        assert_eq!(filled, generator.tile(coord));
    }
}
