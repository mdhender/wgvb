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

Phases 1 through 3 of `DESIGN.md` section 32 are implemented.

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
and phase 4 decides what the elevation bias is worth.

Layers available now are `continentalness`, `regional`, `local`, `detail`,
`elevation-raw`, and `region-influence`. Output is diagnostic: it is driven by
an in-memory generator and does not represent a saved world, and
`elevation-raw` is the unshaped multi-scale composite rather than the elevation
of a `Tile` — the region bias is not folded into it until phase 4.

Terrain classification is not implemented. Elevation is phase 4, climate phase
5, terrain phase 6, and persistence phase 7; `DESIGN.md` is the specification
and section 32 has the ordered phase plan.

## Looking at a seed

`wgvb-map` renders one window to one file, which is right for recording a
golden image and tedious for exploring. `wgvb-serve` puts the same renderer
behind a URL:

```sh
cargo run --release -p wgvb-serve
# then open http://127.0.0.1:8080/seed/0123456789abcdef
```

The seed is sixteen hexadecimal digits in the route, the view center is in the
query string, and six links move the view by whole hexes — `rows / 2` north and
south, `cols / 2` on the four diagonals, so a step means the same thing at every
zoom and a step followed by its opposite returns exactly where it started. There
is no JavaScript and no client-side panning: every state the viewer can be in is
a URL, so a window worth arguing about is a link somebody can paste into an
issue.

It binds loopback by default, clamps the window before rendering it, and its
output is diagnostic in exactly the sense `wgvb-map`'s is — an in-memory
generator from the seed in the route, not a saved world. See `DESIGN.md`
section 29.1.

Continuous fields are not yet periodic across the wrapped edges. That is an
accepted world-warp seam under section 7.1, documented and measured in
`crates/wgvb/tests/wrap_seam.rs`.

## Workspace

| Crate         | Purpose                                                  |
|---------------|----------------------------------------------------------|
| `wgvb`        | Core generator. Stateless, no persistence, no rendering. |
| `wgvb-store`  | Single-world SQLite persistence.                         |
| `wgvb-render` | Bounded viewport rendering to PNG.                       |
| `wgvb-map`    | Diagnostic and player-facing CLI.                        |
| `wgvb-serve`  | Local web viewer for one seed.                           |

## Documents

- `DESIGN.md` — full design specification.
- `CLAUDE.md` — working rules for contributors and coding agents.

WGVB is the Rust successor to [WGVA](https://github.com/mdhender/wgva). It is a
new world format, not a port that reads WGVA files.

## License

MIT. See `LICENSE`.
