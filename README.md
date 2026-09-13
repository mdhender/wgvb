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

Phase 1 of `DESIGN.md` section 32 is implemented: the coordinate and hashing
foundation in `crates/wgvb`. Canonical wrapped coordinates, the six pinned
direction vectors and 60-degree rotation, chunk and region addressing,
axial-to-world conversion, compile-time hash domains with a domain-separated
mixer, and the configuration and generator skeleton with validation.

Nothing generates terrain yet. Continuous scalar fields and the diagnostic
renderer are phase 2; `DESIGN.md` is the specification and section 32 has the
ordered phase plan.

## Workspace

| Crate         | Purpose                                                  |
|---------------|----------------------------------------------------------|
| `wgvb`        | Core generator. Stateless, no persistence, no rendering. |
| `wgvb-store`  | Single-world SQLite persistence.                         |
| `wgvb-render` | Bounded viewport rendering to PNG.                       |
| `wgvb-map`    | Diagnostic and player-facing CLI.                        |

## Documents

- `DESIGN.md` — full design specification.
- `CLAUDE.md` — working rules for contributors and coding agents.

WGVB is the Rust successor to [WGVA](https://github.com/mdhender/wgva). It is a
new world format, not a port that reads WGVA files.

## License

MIT. See `LICENSE`.
