# WGVB Project Guidance

WGVB is a deterministic, effectively unbounded procedural hex-world generator
written in Rust. It is the successor to the Go project WGVA — a new world
format, not a port that reads WGVA files.

**Read `DESIGN.md` before changing generator behavior or public APIs.** Read
section 25 before writing any floating-point code.

## Core invariants

- Preserve `Tile = F(seed, coordinate, algorithm version, configuration)`.
  Generation order, thread scheduling, caches, explored area, and persisted
  mutable state must not affect the generated baseline world.
- Keep the core generator stateless. Persistence and rendering are application
  layers around it, enforced by the dependency graph (see Workspace below).
- Keep raw scalar fields separate from game-facing classification. Never
  generate terrain by independent per-tile random selection.
- Use domain-separated coordinate hashing and deterministic continuous fields.
  No mutable PRNG in the generation path. The `rand` crate is permitted only
  for tooling and test data, never for world generation.
- Treat changes to hashes, field composition, thresholds, defaults, the
  coordinate-to-world conversion, or classification as algorithm compatibility
  changes. Bump `ALGORITHM_VERSION` or deliberately preserve the old
  implementation.
- Support negative coordinates correctly. Use `div_euclid` / `rem_euclid` with
  positive divisors — never bare `/` and `%` — and keep the exhaustive
  around-zero tests even though the helper functions are gone.
- `Component` is `i16` and the world radius derives from it. Compute `s`,
  mirror centers, differences, and every normalization intermediate in `i64`.
  Widening to `i32` changes topology and requires a new algorithm version.
- Follow the Red Blob Games six-mirror-center scheme for hexagonal wraparound,
  computed arithmetically. Never build a mirror lookup table; the map has
  3.2 billion tiles.
- Make procedural fields periodic across wrapped edges on a best-effort basis.
  Document and test any remaining discontinuity as a world-warp seam rather
  than delaying the first implementation.

## Determinism rules (DESIGN.md section 25)

These are the rules most likely to be violated by code that looks correct.

- **The generation path uses only `+`, `-`, `*`, `/`, `sqrt`, `floor`, `abs`,
  `min`, `max`, and comparisons on `f64`.** No `sin`, `cos`, `exp`, `powf`,
  `ln`. Rust routes those to the platform libm, which is not bit-identical
  across targets. Use polynomials; store ridge orientation as a unit vector,
  not an angle.
- **Never call `f64::mul_add` in the generation path.** Rust does not contract
  `a * b + c` into an FMA, and that is exactly the property that makes
  cross-target reproducibility achievable. Using `mul_add` in some places and
  not others reintroduces the Go hazard by hand.
- **Every hash mixer operation uses `wrapping_mul` / `wrapping_add` /
  `wrapping_xor`.** Plain `*` and `+` compile fine and panic on the first
  `cargo test` run in a debug build. This is the most common Go-to-Rust port
  defect.
- **Never use `std::collections::hash_map::DefaultHasher`** for a fingerprint,
  a persisted cache key, or anything in the generation path. Its algorithm is
  explicitly unstable across Rust releases: it passes today and invalidates
  every world on a toolchain upgrade. Use `sha2` for fingerprints and the
  owned mixer in `hash.rs` for everything else.
- **Fixed accumulation order.** Floating-point addition is not associative.
  Iterate directions `0..6` and octaves coarse-to-fine, always. Never reduce
  over a `HashMap` or `HashSet`; sort a `Vec<Coord>` instead — `Coord` is four
  bytes and derives `Ord`.
- Do not add any profile or compiler flag that relaxes floating-point
  semantics. Do not use `-C target-cpu=native` for a build compared against
  goldens.

## Coordinates

- `Coord` has **private fields** and no constructor that skips normalization.
  This is deliberate and load-bearing: it makes "values that normalize to the
  same coordinate identify the same tile" a property of the type rather than a
  convention, so the derived `Eq`, `Hash`, and `Ord` are correct for tile
  identity, persistence keys, and region lookup. Do not add a public
  constructor, a `pub` field, or a `From<(i16, i16)>` that bypasses
  `Coord::new`.
- Coordinate-producing operations such as `neighbor` normalize before
  returning.

## Hex geometry

- The core `wgvb` crate has **no hex library dependency**. It needs only the
  six direction vectors and the `f64` axial-to-world conversion, both pinned by
  the algorithm version.
- `wgvb-render` uses `hexx` for layouts, polygon corners, hit testing, and
  traversal. `hexx` layout math is `f32` via `glam` — fine for pixels,
  unacceptable for the `f64` canonical world space. Keeping `hexx` out of the
  core crate makes that mistake structurally impossible; do not undo it.
- Convert `Component` to `hexx::Hex` through one adapter function.

## Noise and fields

- **Implement the noise in this crate.** Do not add a noise dependency. A
  dependency's semver-minor release can change output and silently invalidate
  every world with no version bump on our side. Anything with runtime CPU
  feature dispatch (`simdnoise`) produces different results on different
  machines and breaks the core invariant outright.
- Field composition is an `enum Field` with `#[inline]` match dispatch, not
  `Box<dyn Field2D>`. The set of field kinds is closed and comes from config,
  so an enum inlines fully, carries no vtable, and serializes into the stored
  configuration.

## Configuration

- `#[serde(deny_unknown_fields)]` on `Config` is required: an older binary must
  reject a newer world file, not silently ignore fields.
- **Never `#[serde(default)]` a field that affects generation.** A defaulted
  missing field is a changed world with an unchanged version number. Adding
  such a field is an algorithm version change.
- Fingerprint is SHA-256 over `algorithm_version_le_bytes || canonical CBOR`
  (`ciborium`). Hash `f64` as `to_bits()`, normalize `-0.0`, reject `NaN`
  during validation.
- Scale fields name their unit in the identifier: `_wavelength_miles`,
  `_hexes`. Validation rejects non-finite, non-positive, and out-of-range
  values.

## Concurrency

- `Generator` is immutable and `Sync`, checked by a `const` assertion in the
  test module, not promised in a comment. Adding interior mutability to
  `Generator` breaks that assertion at compile time — that is the point.
- `unsafe_code = "forbid"` at the workspace level. There is no reason for
  `unsafe` in a pure-function world generator.
- `rayon` may parallelize batch fills because each tile is a pure function of
  its own coordinate written to its own slot. **Never add a batch operation
  that accumulates across tiles** — a sum, min/max, or histogram — because the
  result would depend on the work-stealing split.
- Do not call `tile()` from inside `tile()`. Both `tile` and `relief` call a
  private `elevation_scalar`.

## Persistence

- `rusqlite` with the `bundled` feature. No ORM, no query builder, no
  connection pool until profiling justifies one.
- Application id is `0x5747_5642` (ASCII `"WGVB"`). This differs from WGVA's
  `0x5747_5641`; a WGVA file must be rejected at the first gate.
- Every coordinate-keyed table is `WITHOUT ROWID` with `PRIMARY KEY (q, r)`.
  That makes it a B-tree keyed by coordinate, so viewport and chunk loads are
  one ordered range scan.
- Migrations are an owned `user_version` ladder in `crates/wgvb-store`. Add
  migrations in order; **never edit a released migration.**
- No foreign keys to generated data. Authoritative player state must not have a
  referential dependency on a discardable cache. Still enable
  `PRAGMA foreign_keys = ON` on every connection.
- One database, one world. No `world_id` column.
- Generated tiles, chunks, and PNGs are reproducible caches, not authoritative
  records. If cached, validate against the configuration fingerprint and the
  render version. **Do not build the tile cache in the first implementation;
  measure first.**
- Each opening gate is one `OpenError` variant. Tests assert on variants, never
  on message strings. No gate performs an application write before it passes.

## Rendering

- Rendering is bounded even though generation is effectively unbounded. Every
  render request defines a finite viewport and explicit pixel scale. Renderer
  pixel coordinates never feed back into generation.
- Render coordinates in a stable sorted order; overlapping edges and labels
  make output order-dependent otherwise.
- Keep generated terrain separate from player overlays (discoveries, fog of
  war, settlements, labels, annotations) and compose at render time.
- **Golden-compare decoded RGBA buffers, not PNG file bytes.** The `png`
  crate's default filter and compression settings can change between versions.
- `wgvb-serve` is a front end, not a second renderer. Its window arithmetic is
  `Viewport::centered_on`; no offset-coordinate arithmetic lives in the server,
  and a test asserts that its PNG bytes are identical to `wgvb-map`'s for the
  same window. Scroll steps are counted in tiles, never in pixels. See
  `DESIGN.md` section 29.1.

## Public generated data

- A normal `Tile` carries normalized elevation, heat, moisture, and relief
  values *in addition to* the elevation, climate, and terrain classifications.
  These are not diagnostics-only data.
- Climate is independent `HeatBand` and `MoistureBand`. Do not create one enum
  mixing temperature and moisture.
- Pin every enum discriminant explicitly (`#[repr(u8)]`, `Ocean = 0, ...`).
  These values are persisted; reordering variants silently changes stored data.
- Generate deterministic basin influence even when coherent inland water is not
  feasible. Emit lakes or inland seas only when bounded local generation gives
  consistent membership, surface elevation, depth, and shorelines; otherwise
  omit inland water from the first version.

## Workspace

```text
wgvb-map    ->  wgvb-render  ->  wgvb
    |               |
    +-> wgvb-store ----------->  wgvb

wgvb-serve  ->  wgvb-render  ->  wgvb
```

- `crates/wgvb` depends on exactly `serde` and `thiserror`. **Do not add a
  dependency to the core crate without stating why in the commit message.** The
  one-directional graph is what makes "persistence is not in the core" a
  compile-time fact instead of advice.
- All dependency versions are pinned once in the workspace root
  `[workspace.dependencies]`. Member crates opt in with `dep.workspace = true`.
- Rust module privacy replaces Go's `internal/`. A module is private unless
  declared `pub`; use `pub(crate)` for the rest.

## Verification

- Run `cargo fmt`, `cargo clippy --workspace --all-targets`, and
  `cargo test --workspace` before reporting work complete.
- Derive expected test values independently rather than recording whatever the
  code currently produces.
- Test determinism, order independence, concurrent generation, negative
  coordinates, all six wrapped edges, region and chunk continuity, and every
  database compatibility gate.
- Add golden coordinates only for an intentionally stable algorithm version.
  Updating a golden world requires an explicit compatibility decision recorded
  in the commit message. Run goldens on more than one target — that is the only
  thing that actually proves the determinism rules are being honored.
- Run focused benchmarks when changing generation hot paths or caches. Measure
  before optimizing.
