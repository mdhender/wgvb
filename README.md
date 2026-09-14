# WGVB

Deterministic, effectively unbounded procedural hex-world generator.

WGVB generates attractive, geographically coherent terrain on demand without
exposing a practical map boundary. Any tile can be generated independently:

```text
(seed, q, r) -> tile
```

Adjacent tiles nonetheless participate in coherent oceans, continents, mountain
belts, climate zones, and biomes, because terrain is derived from continuous
multi-scale noise fields and blended hierarchical regions rather than from an
independent per-tile hash.

The world is a wrapped hexagonal address space of 3,221,127,169 tiles at a
3-mile apothem — roughly 510 Earth surface areas. Players never reach an edge.

## Status

Phases 1 through 7 of `DESIGN.md` section 32 are implemented.

**Phase 1 — coordinates and hashing.** Canonical wrapped coordinates, the six
pinned direction vectors and 60-degree rotation, chunk and region addressing,
axial-to-world conversion, compile-time hash domains with a domain-separated
mixer, and the configuration and generator skeleton with validation.

**Phase 2 — continuous scalar fields and the diagnostic renderer.** Owned 2D
noise — value noise with quintic interpolation, and simplex noise on a
triangular lattice — with no noise dependency. A `Field` enum composes fbm,
domain warping, and weighted sums, and `Generator::sample` returns the
multi-scale fields at any canonical coordinate. `wgvb-map` draws a bounded
window of any of them to a PNG.

```sh
cargo run --release -p wgvb-map -- \
    --seed 20260912 --q 0 --r 0 --cols 300 --rows 220 \
    --hex-radius 3 --layer elevation-raw --out map.png
```

**Phase 3 — hierarchical region influence.** `Generator::region_params` gives
any coordinate a deterministic elevation, moisture, heat, roughness, basin,
volcanic, and variation bias plus a ridge orientation, derived by hashing anchor
addresses and blended across the anchors of a macro-region and a region level.
Nothing is stored and nothing is cached. The blend follows the triangular anchor
lattice rather than the parallelogram addressing cell, so distant areas have
distinct character with no visible lattice — draw `--layer region-influence`
over a wide window to see it. Regions bias fields; they do not assign terrain,
and each consuming phase decides what a bias is worth.

**Phase 4 — elevation.** `Generator::tile` returns a `Tile` with a normalized
elevation, a local relief estimated from the six neighboring elevations, and an
`Elevation` band. The composite adds region uplift, a ridged structure term
elongated along the region's ridge orientation, a sea-level offset, and a
contrast shaping pass, all from configuration. Sea level and the band
thresholds are thresholds on a globally stable field, never quantiles of a
generated sample: the elevation of a tile does not depend on which other tiles
have been generated.

**Phase 5 — climate.** Every tile also carries a normalized heat and moisture
value and a `Climate` of two independent bands, `HeatBand` and `MoistureBand`.
Temperature is a broad heat field plus the region's heat bias less a cooling
proportional to height above sea level; moisture is a broad field, the region's
moisture bias, and a shorter-wavelength local variation. The two axes stay
independent — cold and arid are not the same thing, and a cold rainforest has to
be expressible. The wrapped world has no equator, so the heat zones are
procedural rather than latitudinal.

```sh
cargo run --release -p wgvb-map -- \
    --seed 81985529216486895 --q -600 --r -450 --cols 1201 --rows 901 \
    --hex-radius 1 --layer climate --out climate.png
```

Climate is built at a scale a window has to be wide to show — a zone is
thousands of hexes across — so the contact sheets in `docs/renders/v4` are the
evidence phase 5 met its exit condition, alongside the measurements in
`crates/wgvb/tests/climate.rs`.

**Phase 6 — basins and terrain.** `Tile` is complete: every tile now carries a
`Terrain` as well, classified from elevation, relief, climate, basin influence,
volcanic tendency, and the six neighboring elevations — never from a per-tile
draw. Basin influence is three deterministic fields at broad, regional, and
local scale blended with the region's basin bias, using bounded local sampling
and no connectivity or flood fill anywhere. Terrain reads it through a product,
`moisture + weight * basin * moisture`, so a basin makes a wet climate wetter
and a dry one drier and an endorheic basin is a salt flat rather than a lake.

```sh
cargo run --release -p wgvb-map -- \
    --seed 81985529216486895 --q 11000 --r -4500 --cols 1201 --rows 901 \
    --hex-radius 1 --layer terrain --out terrain.png
```

**Inland water is deliberately omitted.** No world this version generates
contains a `Lake` or an `InlandSea`. A coherent lake needs one surface
elevation shared by every tile of one basin, and knowing which tiles those are
is connectivity — which sections 2.3 and 15 forbid. `DESIGN.md` section 17.1 is
the decision record, with the two approximations that were considered and
rejected. The variants stay in the vocabulary with their discriminants pinned.

Layers available now are `continentalness`, `regional`, `local`, `detail`,
`elevation-raw`, `elevation`, `relief`, `ridge`, `roughness`,
`region-influence`, `temperature`, `moisture`, `climate`, `basin`, `volcanic`,
and `terrain`. Output is diagnostic: it is driven by an in-memory generator and
does not represent a saved world, and `elevation-raw` is the unshaped
multi-scale composite rather than the elevation of a `Tile`.

The contact sheets in `docs/renders/v5` are the evidence phase 6 met its exit
condition, alongside the measurements in `crates/wgvb/tests/terrain.rs` and
`crates/wgvb/tests/basin.rs`.

**Phase 7 — persistence, player rendering, and tuning.** A world lives in one
SQLite database that holds the seed, the algorithm version, the complete
effective configuration, and that configuration's SHA-256 fingerprint. Opening
one runs the six gates of `DESIGN.md` section 27.5 in order, and no gate writes
before it passes: a file that is not a WGVB database, a schema newer than the
binary, a generator version this binary cannot reproduce, and a configuration
that has been edited away from its fingerprint are each a distinct typed error
rather than a world quietly regenerated under current rules.

Player state — what has been seen, what has been built — is stored apart from
generated terrain as sparse `WITHOUT ROWID` overlays keyed by `(q, r)`, and the
two meet only at render time. The renders in `docs/renders/player` are the
evidence phase 7 met its exit condition. See "Saving a world" below.

`DESIGN.md` is the specification and section 32 has the ordered phase plan.

## Saving a world

`--seed` on its own renders a diagnostic picture of a world nobody has saved.
`--db` renders a world that exists:

```sh
# Creates world.wgvb from the seed, then renders a window of it.
cargo run --release -p wgvb-map -- \
    --db world.wgvb --seed 81985529216486895 \
    --q -600 --r -450 --cols 400 --rows 300 --hex-radius 3 \
    --layer terrain --out map.png

# Every later run takes the seed and the configuration from the file.
cargo run --release -p wgvb-map -- \
    --db world.wgvb --q 11000 --r -4500 --cols 400 --rows 300 \
    --hex-radius 3 --layer terrain --out far.png
```

The database is authoritative for the world and the command line is
authoritative only for the window. Creating one writes the *complete* effective
configuration, including every value that came from a default, before anything
is rendered; reopening never substitutes current program defaults for a missing
stored value, and naming a `--seed` the file disagrees with is an error rather
than a silent override.

Player overlays are written by the same command and composed at render time:

```sh
cargo run --release -p wgvb-map -- \
    --db world.wgvb --discover -600,-450 --discover-radius 40 \
    --settle -600,-450=Ashford \
    --q -700 --r -520 --cols 400 --rows 300 --hex-radius 3 \
    --layer terrain --out explored.png
```

Undiscovered ground is drawn as fog; a settlement is drawn through it, because
it is something the player built. A world that records no discoveries at all is
one where exploration is not being tracked, so nothing is hidden. See
`DESIGN.md` section 29.2.

There is deliberately **no tile cache**: section 27.6 asked for a measurement
before building one, and `crates/wgvb/tests/bench.rs` is that measurement.
Section 31 says how to run it and what has to be held fixed for two numbers to
be comparable:

```sh
cargo test --release -p wgvb --test bench -- --ignored --nocapture
```

## Looking at a seed

`wgvb-map` renders one window to one file, which is right for recording a
golden image and tedious for exploring. `wgvb-serve` puts the same renderer
behind a URL:

```sh
cargo run --release -p wgvb-serve
# then open http://127.0.0.1:8080/seed/0123456789abcdef
```

Point it at a world and it serves that world instead:

```sh
cargo run --release -p wgvb-serve -- --db world.wgvb
```

The database then supplies the seed, the algorithm version, and the complete
effective configuration, and **the seed in the route becomes a check against the
stored one** rather than the source of it — asking for another seed is a 404
naming the one the server holds. Player overlays are read fresh on every
request, so exploring with `wgvb-map --discover` and refreshing the page shows
the exploration. The server opens a world and never creates one; creating is
`wgvb-map --db`'s job, because creating a world is a decision rather than a
side effect of a typo.

The seed is sixteen hexadecimal digits in the route, the view center is in the
query string, and six links move the view by whole hexes — `rows / 2` north and
south, `cols / 2` on the four diagonals, so a step means the same thing at every
zoom and a step followed by its opposite returns exactly where it started. There
is no JavaScript and no client-side panning: every state the viewer can be in is
a URL, so a window worth arguing about is a link somebody can paste into an
issue.

Every layer has a key, and the page names the tile at its center — the
terrain, the elevation band, the two climate bands, and the scalars they were
classified from — so a link to a window says what is in it rather than leaving
a reader to count swatches.

It binds loopback by default and clamps the window before rendering it. Without
`--db` its output is diagnostic in exactly the sense `wgvb-map`'s is — an
in-memory generator from the seed in the route, not a saved world — and the page
says which of the two it is rather than always claiming the first. See
`DESIGN.md` section 29.1.

## Tuning a world

`wgvb-serve` shows you a world. `wgvb-tune` lets you change one:

```sh
cargo run --release -p wgvb-tune
# then open http://127.0.0.1:8081/seed/0123456789abcdef
```

Three tabs, all of them links you can paste. **map** is the viewer's window,
with the compass, the layers, and the center-tile readout. **grid** draws one
pixel per hex — a thousand tiles on a side by default, about a second of eight
cores — so a continent can be judged in one look. **config** is the complete
effective configuration as a form: change a field, apply it, and the map redraws
under it.

The map tab also counts what is in the window — the terrain mix, the elevation
bands, and the two climate ladders — so a threshold move can be read as a number
and not only squinted at. Moving `dry_level` from `-0.1` to `0.0` takes one
coastal window from 6% dry to 62%, and from 70% plains to 33% plains and 38%
grassland.

```sh
# start from a configuration you saved earlier
cargo run --release -p wgvb-tune -- --config tuned.toml --seed feedface

# draw what you tuned, from the command line
cargo run --release -p wgvb-map -- --config tuned.toml --seed 7 \
    --q 0 --r 0 --cols 1001 --rows 1001 --grid 1 --layer terrain --out world.png
```

The configuration is held in memory, not in the URL — a hundred numeric fields
do not fit in an address bar, which is why this is a second tool rather than a
mode of the viewer. Every page prints the configuration's fingerprint, the
config tab downloads the exact file behind it, and `wgvb-map --config` renders
that file, so a picture can always be traced back and reproduced. **It cannot
open, create, or write a world**: `wgvb-store` is not in its dependency graph.

Each render logs what it cost — tiles, milliseconds, tiles per second — because
measuring that is the other half of tuning:

```text
wgvb-tune: GET /seed/0123456789abcdef/grid.png?... 200
    (1002001 tiles, 405 ms generate, 190 ms encode, 2471232 tiles/s)
```

Windows are bounded by a budget counted in generator evaluations rather than
tiles, since `relief`, `climate`, and `terrain` cost seven apiece; `--budget`
raises it. See `DESIGN.md` section 29.3.

Continuous fields are not yet periodic across the wrapped edges. That is an
accepted world-warp seam under section 7.1, documented and measured in
`crates/wgvb/tests/wrap_seam.rs`.

## Workspace

| Crate         | Purpose                                                  |
|---------------|----------------------------------------------------------|
| `wgvb`        | Core generator. Stateless, no persistence, no rendering. |
| `wgvb-store`  | Single-world SQLite persistence.                         |
| `wgvb-config` | Canonical bytes, fingerprint, and the TOML config file.  |
| `wgvb-render` | Bounded viewport rendering to PNG.                       |
| `wgvb-view`   | The URL grammar both web front ends present.             |
| `wgvb-map`    | Diagnostic and player-facing CLI.                        |
| `wgvb-serve`  | Local web viewer for one seed or one saved world.        |
| `wgvb-tune`   | Local web instrument for tuning a configuration.         |

## Documents

- `DESIGN.md` — full design specification.
- `CLAUDE.md` — working rules for contributors and coding agents.

WGVB is the Rust successor to [WGVA](https://github.com/mdhender/wgva). It is a
new world format, not a port that reads WGVA files.

## License

MIT. See `LICENSE`.
