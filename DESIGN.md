# WGVB — Effectively Unbounded Procedural World Generator Design

**Repository:** `github.com/mdhender/wgvb`
**Crate:** `wgvb`
**Target language:** Rust (edition 2024, `rust-version = "1.98"`)
**Coordinate system:** axial hex coordinates `(q, r)`
**Hex scale:** 3-mile apothem
**World origin:** `(0, 0)`
**Primary goal:** Generate attractive, geographically coherent terrain on demand without exposing a practical map boundary.

WGVB is the Rust successor to WGVA. It is a new world format, not a port that
can read WGVA databases: the application id, the crate name, and the algorithm
version namespace are all distinct.

---

## 1. Purpose

WGVB is a deterministic procedural world generator intended for games that need an effectively unbounded hex map.

The generator must preserve the strongest property of a simple coordinate-hash world:

```text
(seed, q, r) -> tile
```

A tile can be generated independently at any coordinate without first constructing or storing the rest of the map.

Unlike a naive coordinate-hash generator, however, adjacent tiles must participate in coherent geographic structures such as:

- oceans and continental interiors,
- mountain belts,
- uplands and lowlands,
- climate zones,
- forests, plains, deserts, marshes, and other terrain,
- region-scale geographic variation that crosses chunk boundaries naturally.

The implementation uses a finite but enormous wrapped hexagonal address space. This is described as "unbounded" to players because normal play should never encounter a terminal edge. Its large-scale geography is generated from deterministic continuous fields and hierarchical regions.

---

## 2. Design Goals

### 2.1 Deterministic

For a given world seed and tile coordinate, generated attributes must always be identical.

```rust
generator.tile(c) == generator.tile(c)
```

Generation order must not affect results.

### 2.2 Effectively unbounded and wrapped

No API should require world width, height, radius, or bounding rectangle.

For the alpha generator, canonical coordinates form a hexagonal map with signed 16-bit cube components. Coordinate operations that leave that map wrap to the corresponding tile on the opposite edge using the scheme in section 7.1.

### 2.3 Local

Generating tile `(q, r)` should require only a bounded amount of nearby procedural context.

Generation must not require:

- generating every tile between `(0,0)` and `(q,r)`,
- scanning the entire world,
- global normalization,
- global flood fill,
- a precomputed heightmap.

### 2.4 Geographically coherent

Nearby tiles should normally have related elevation, climate, and terrain.

Large features should cross implementation chunk and region boundaries without visible seams.

### 2.5 Reproducible across platforms

The same seed and coordinates must produce bit-identical results on every supported target.

Rust makes this a guarantee rather than an aspiration, but only under the arithmetic restrictions in section 25. Read that section before writing any field code.

Output must never depend on:

- hash map iteration order,
- platform floating-point library differences,
- thread scheduling or work-stealing order,
- mutable random-number-generator state,
- compiler or dependency version, within a fixed algorithm version.

### 2.6 Stateless by default

The core generator requires no persistent storage. This is enforced structurally: the `wgvb` crate does not depend on `wgvb-store`.

Applications may cache generated tiles or regions, but caching must be an optimization, not part of correctness.

---

## 3. Non-Goals

The first implementation does **not** need to guarantee:

- an exact global percentage of land versus water,
- a fixed number of continents,
- globally correct river drainage networks,
- plate tectonic simulation,
- erosion simulation,
- settlement placement,
- roads,
- political boundaries,
- resource placement,
- historical simulation.

---

## 4. Core Model

A tile is uniquely identified by its canonical axial coordinate.

```rust
pub type Component = i16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Coord {
    q: Component,
    r: Component,
}
```

**`Coord` fields are private and there is no public constructor that skips
normalization.** This is the single most important structural difference from
the Go design. In Go, `Coord{Q: 40000, R: 40000}` is a valid literal anywhere in
the program, and "values that normalize to the same coordinate identify the same
tile" (section 7.1) can only be upheld by discipline. Here, a non-canonical
`Coord` cannot be constructed at all, so the derived `PartialEq`, `Hash`, and
`Ord` implementations are automatically correct for tile identity, persistence
keys, region lookup, and set membership.

The only ways to obtain a `Coord`:

```rust
impl Coord {
    /// Normalizes any axial pair into the canonical wrapped map.
    pub fn new(q: i64, r: i64) -> Coord;

    /// The canonical origin.
    pub const ORIGIN: Coord;

    pub fn q(self) -> Component;
    pub fn r(self) -> Component;
    /// Derived cube component, `-q - r`. Always in `Component` range for a
    /// canonical coordinate.
    pub fn s(self) -> Component;
}
```

Store axial coordinates as signed 16-bit `Component` values. Compute `s`, mirror
centers, differences, and every other intermediate with `i64`. Not every pair of
signed 16-bit `q` and `r` has an `s` in the signed 16-bit range; normalization
handles that, and the conversion back to `Component` uses `i16::try_from` on a
value the normalizer has already proven in range, so the `expect` documents an
invariant rather than hoping for one.

Keep the component type and world-radius constant centralized in `lib.rs`.
Widening to `i32` should require changing those definitions and the
compatibility tests, not rewriting algorithms or the database schema. The
widening still changes world topology and therefore requires a new algorithm
version.

### 4.1 Tile

The minimum public tile representation is:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    pub coord: Coord,

    pub elevation_value: f64,
    pub heat_value: f64,
    pub moisture_value: f64,
    pub relief_value: f64,

    pub elevation: Elevation,
    pub climate: Climate,
    pub terrain: Terrain,
}
```

`Tile` is `Copy` and small enough (under 64 bytes) that batch APIs can fill a
`&mut [Tile]` with no allocation and no indirection.

The physical values and their elevation, climate, and terrain classifications are part of the ordinary tile result. Games and renderers must not need a diagnostic API to recover them.

The generator may also expose intermediate values for diagnostics:

```rust
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub coord: Coord,

    pub continentalness: f64,
    pub regional_uplift: f64,
    pub basin_influence: f64,

    pub tile: Tile,
}
```

Diagnostic values are not required for ordinary game use.

---

## 5. Recommended Architecture

WGVB uses four conceptual layers:

```text
world seed
    |
    +-- macro-scale continuous fields
    |
    +-- deterministic hierarchical regions
    |
    +-- local detail fields
    |
    +-- classification
            |
            +-- elevation
            +-- climate
            +-- terrain
```

The important architectural rule is:

> Chunks and regions are addressing and organization devices, not visible geographic boundaries.

Noise fields and region influences must extend across them.

---

## 6. Generation Pipeline

For a requested tile `(q, r)`:

```text
1. Normalize the coordinate into the wrapped canonical map.
2. Convert the canonical coordinate to continuous world-space position.
3. Evaluate macro-scale fields.
4. Determine hierarchical regional influences.
5. Evaluate medium- and local-scale detail.
6. Combine fields into normalized physical values.
7. Classify elevation.
8. Classify climate.
9. Classify terrain.
10. Return immutable tile data with its canonical coordinate.
```

Conceptually:

```rust
impl Generator {
    pub fn tile(&self, c: Coord) -> Tile;
}
```

should be sufficient for callers. Note `&self`: generation never mutates the generator. See section 22.

---

## 7. Hex Coordinates and World Space

WGVB uses axial hex coordinates `(q, r)`.

Axial coordinates define tile identity and adjacency, not how hexes must be drawn. Flat-top versus pointy-top orientation is a rendering and layout choice; it is not part of the generator's public model. A renderer may choose either orientation without changing generated tile data.

Noise functions operate on Cartesian coordinates, so axial coordinates are converted to continuous 2D world space before sampling.

Each regular hex has a 3-mile apothem. Therefore:

```text
neighboring center distance = 6 miles
flat-to-flat width          = 6 miles
point-to-point width        = 4 * sqrt(3) miles, approximately 6.93 miles
area                        = 18 * sqrt(3) square miles, approximately 31.18 square miles
```

Canonical world-space coordinates are measured in miles, in `f64`.

One convenient pointy-top embedding:

```text
x = 6 * (q + r/2)
y = 3 * sqrt(3) * r
```

This places adjacent hex centers exactly 6 miles apart. Any equivalent embedding
is acceptable if it preserves that distance and uses miles. The chosen embedding
is an internal detail but must remain stable wherever deterministic
compatibility is promised, so it is pinned by the algorithm version.

Centralize the conversion:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

pub fn axial_to_world(c: Coord) -> Vec2;
```

All continuous fields sample from this one coordinate system. This avoids the distortion caused by feeding `q` and `r` directly into Cartesian noise.

### 7.1 Wrapped coordinate domain

For the alpha generator, let `N = i16::MAX`. The canonical map is the hexagonal cube-coordinate domain:

```text
-N <= q <= +N
-N <= r <= +N
-N <= s <= +N
q + r + s = 0
```

Wrapping follows the hexagonal wraparound construction described by Red Blob Games. The six mirror centers are the rotations of:

```text
(2*N+1, -N, -N-1)
```

These are `const` data, computed at compile time, generated by the same `rotate_once` used for player rotation (appendix A):

```rust
const MIRROR_CENTERS: [(i64, i64, i64); 6] = /* rotations of (2*N+1, -N, -N-1) */;
```

When an operation produces a coordinate outside the canonical map, translate it by the appropriate mirror center until it is canonical. The implementation must use arithmetic normalization rather than a precomputed mirror table because this map is far too large to enumerate. Use `i64` for mirror centers and every normalization intermediate, then convert canonical `q` and `r` to `Component`.

All public coordinate operations, neighbor sampling, persistence keys, region lookup, and rendering use the same canonicalizer. Because `Coord` cannot be constructed without it (section 4), this is structural rather than advisory.

The implemented normalizer runs in two stages, because `Coord::new` accepts any
`i64` pair and stepping one mirror center at a time would need about `1.4e14`
translations for an input near `i64::MAX`:

1. **Lattice solve.** Mirror centers `0` and `1` generate the wraparound lattice.
   Inverting that two-vector basis gives the multiples directly; rounding each to
   the nearest integer leaves a residual within hex distance `2N+1` of the origin.
2. **Greedy fix-up.** Subtract whichever mirror center most reduces
   `max(|q|, |r|, |s|)`, in fixed order `0..6`, until none does. The six centers
   are the Voronoi-relevant vectors of the lattice and the canonical hexagon is an
   exact fundamental domain of it — `1 + 3N(N+1)` tiles for a lattice of the same
   index — so every coordinate has exactly one canonical representative, there is
   no tie to break, and a point no center improves is already canonical.

The already-canonical case short-circuits before either stage, which is the
overwhelmingly common one.

> **The lattice solve is the one place that widens past `i64`.** Products such as
> `(2N+1) * q` reach `9.2e18` for `|q|` near `i64::MAX`, at the very top of the
> `i64` range. The rule in section 4 exists to prevent silent wraparound, so the
> solve is computed in `i128` and the small residual returns to `i64`. Mirror
> centers, differences, and every other intermediate stay `i64`. Do not "simplify"
> the solve back to `i64`.

WGVB should make a best effort to make continuous fields periodic under the mirror translations so terrain joins naturally across wrapped edges. Exact periodicity must not delay the first implementation. If a field cannot be made periodic without disproportionate complexity or loss of quality, the discontinuity is an accepted world-warp seam and must be documented and tested as such.

**Accepted seam, as implemented.** The phase 2 fields are *not* periodic under
the mirror translations, and that is the accepted state rather than an
oversight. Sampling is a pure function of the canonical coordinate, so a
coordinate and its wrapped image produce bit-identical values — that much is
exact and is what tile identity depends on. But two tiles that neighbor each
other *across* a wrapped edge lie roughly `393,204` miles apart in canonical
world space, so their field values are uncorrelated and the join is visible.

Closing that join needs a noise lattice whose spacing divides the wrap period.
That period is `2N+1 = 65535 = 3 * 5 * 17 * 257` hexes, so every wavelength in
the configuration — and every fbm octave derived from it through the
lacunarity — would have to be drawn from the divisors of `65535`. The
constraint is real and joint, and it cuts into the multi-scale table of
section 10, so it is deferred rather than rushed. Taking it later is an
algorithm compatibility change under section 27.

Until then the seam is measured, not merely admitted:
`crates/wgvb/tests/wrap_seam.rs` asserts bit-exact wrap consistency across all
six edges and all integer combinations of the mirror centers, asserts that the
discontinuity exists only where it is expected to, and fails loudly if a future
change makes the fields periodic without updating this section.

**Hex geometry dependency.** The core `wgvb` crate does *not* depend on a hex
library. It needs only the six direction vectors and the `f64` axial-to-world
conversion above, both a few lines, and both pinned by the algorithm version.

The `wgvb-render` crate uses [`hexx`](https://crates.io/crates/hexx) for
layouts, polygon corners, hit testing, ring and spiral traversal, and finite-area
iteration. `hexx::Hex` is `i32`-backed, so `Component` converts losslessly
through one adapter function.

> **`hexx` layout math is `f32`.** `hexx` is built on `glam`, and its layout and
> world-position types use `f32` vectors. That is fine for pixel geometry and
> unacceptable for the `f64` canonical world space that fields sample. Keeping
> `hexx` out of the core crate makes this impossible to get wrong by accident:
> generation coordinates cannot be routed through `hexx` layout math because the
> core crate cannot see `hexx`.

`hexx` is Apache-2.0; WGVB is MIT. That combination is fine for distribution.

### 7.2 World scale

For a cube-coordinate hexagon of radius `N`, the number of tiles is:

```text
tiles = 1 + 3*N*(N+1)
```

With `N = i16::MAX`, the alpha world contains exactly `3,221,127,169` canonical tiles, approximately `3.2211e9`.

At a 3-mile apothem, each tile covers `18*sqrt(3)`, approximately `31.1769`, square miles. The alpha's total surface area is therefore approximately `1.00425e11` square miles. The center-to-center radius is `196,602` miles, and the opposite-corner center span is `393,204` miles. This is approximately 510 Earth surface areas: large enough to exercise edge wrapping during alpha testing while remaining effectively unbounded for gameplay.

---

## 8. Coordinate-Based Randomness

WGVB does not use a mutable PRNG stream as the basis of terrain generation.
That would make results depend on traversal order.

The `rand` crate may be used for things that are *not* world generation — test
data, sampling for statistical checks, tie-breaking in tooling. It must never
appear in the generation path.

Derive deterministic values from semantic paths instead:

```text
hash(seed, "macro-elevation", region_q, region_r)
hash(seed, "mountain-axis",   region_q, region_r)
hash(seed, "local-detail",    q, r)
```

Different procedural systems must use independent domains so a change to one field cannot perturb another.

### 8.1 Domains are compile-time constants

Rust `const fn` lets domain identifiers be derived from string literals at
compile time, with no initialization cost and no hand-assigned integers that can
drift or collide:

```rust
/// FNV-1a over the domain name. Stable by construction: the algorithm is
/// written here, so no dependency can change it.
const fn domain(name: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < name.len() {
        hash ^= name[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash
}

pub const DOM_CONTINENTALNESS: u64 = domain(b"continentalness");
pub const DOM_REGIONAL_ELEVATION: u64 = domain(b"regional-elevation");
pub const DOM_RELIEF: u64 = domain(b"relief");
pub const DOM_MOISTURE: u64 = domain(b"moisture");
pub const DOM_TEMPERATURE: u64 = domain(b"temperature");
pub const DOM_TERRAIN_DETAIL: u64 = domain(b"terrain-detail");
pub const DOM_REGION_STYLE: u64 = domain(b"region-style");
pub const DOM_RIDGE_ORIENTATION: u64 = domain(b"ridge-orientation");
pub const DOM_BASIN: u64 = domain(b"basin");
pub const DOM_VOLCANIC: u64 = domain(b"volcanic");
```

Adding a domain never disturbs an existing one. Renaming one changes the world
and is an algorithm version change.

### 8.2 The hash primitive

Rust has no variadics, which is an improvement here: fixed arity makes each call
site explicit about how many coordinates feed the hash.

```rust
pub fn hash2(seed: u64, domain: u64, a: i64, b: i64) -> u64;
pub fn hash3(seed: u64, domain: u64, a: i64, b: i64, c: i64) -> u64;
```

or a const-generic form if more shapes are needed:

```rust
pub fn hash_n<const N: usize>(seed: u64, domain: u64, values: [i64; N]) -> u64;
```

> **Every mixer operation must use `wrapping_mul` / `wrapping_add` / `wrapping_xor`.**
> Go wraps integer arithmetic silently; Rust panics on overflow in debug builds
> and wraps in release. A mixer written with plain `*` and `+` compiles, passes
> nothing, and panics the first time `cargo test` runs it. This is the single
> most common defect when porting hashing code from Go.

Use a well-understood finalizer — SplitMix64 or the `xxh3` avalanche step — written out in this crate rather than pulled from a dependency, for the same reason the noise is (section 9.2). Convert to `f64` in `[0, 1)` by taking the top 53 bits, never by `as f64 / u64::MAX as f64`.

Do not use `std::collections::hash_map::DefaultHasher` anywhere. See section 25.4.

---

## 9. Continuous Noise

The generator uses deterministic continuous noise for spatial coherence.

Suitable families:

- gradient noise,
- simplex-style noise,
- OpenSimplex-style noise,
- value noise with quintic interpolation,
- domain-warped combinations of the above.

The noise must evaluate at arbitrary coordinates with no finite array.

### 9.1 Dispatch: use an enum, not a trait object

The Go design used a `Field2D` interface. Rust offers three options, and the
right one is not the obvious one.

| Option | Cost | Config-driven? |
|---|---|---|
| `Box<dyn Field2D>` | vtable call per octave per tile, no inlining | yes |
| `impl Field2D` generics | fully inlined | no — composition is chosen at runtime from config |
| `enum Field` + `match` | fully inlined, monomorphic | yes |

The set of field kinds is *closed* and comes from configuration, so an enum is
both faster and a better fit:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Field {
    Value { seed: u64, domain: u64, wavelength_miles: f64 },
    Simplex { seed: u64, domain: u64, wavelength_miles: f64 },
    Fbm { source: Box<Field>, octaves: u8, lacunarity: f64, gain: f64 },
    Warp { source: Box<Field>, wx: Box<Field>, wy: Box<Field>, strength_miles: f64 },
    Offset { source: Box<Field>, dx_miles: f64, dy_miles: f64 },
    Sum(Vec<(f64, Field)>),
}

impl Field {
    #[inline]
    pub fn sample(&self, x: f64, y: f64) -> f64 { /* match */ }
}
```

This composition tree inlines, carries no vtables, and serializes directly into
the persisted configuration — so the exact field graph that produced a world is
stored in the world file rather than reconstructed from scattered constants.

Normalized output range is `[-1, +1]`, documented per variant.

### 9.2 Implement the noise in this crate

**Do not take a noise crate as a dependency.** Three reasons:

1. Section 27 requires that any change to a noise formula be an algorithm
   version change. A dependency's semver-minor release can change its output and
   silently invalidate every existing world with no version bump on our side.
2. Crates that dispatch on runtime CPU features — `simdnoise` and anything with
   a `std::is_x86_feature_detected!` path — produce **different results on
   different machines**. That breaks `Tile = F(seed, coord, version, config)`
   outright, not subtly.
3. OpenSimplex2 or value-noise-with-quintic is roughly 200 lines. Owning it means
   the algorithm version genuinely pins the output, which is the whole point.

Put it in a private `noise` module of the `wgvb` crate. Cite the reference
implementation and its license in a module comment.

### 9.3 The octave ladder stops at the tile grid's Nyquist wavelength

Adjacent tile centers are `2 * APOTHEM_MILES` apart, so the shortest feature the
grid can carry is `4 * APOTHEM_MILES` — twelve miles at the alpha scale. An fbm
octave below that limit is not detail. It cannot be represented at all, and what
reaches the tile is an aliased sample of it: per-tile noise, paid for with a
noise evaluation and then thrown away.

Relief is where this shows. Relief is a first difference between neighbors,
which is exactly the operation that amplifies content near the limit, so the
aliased octaves dominate it and the layer reads as speckle with the real ridge
structure buried underneath. Elevation itself survives, because the aliased
amplitude is small against the coarse scales — which is precisely why nothing
catches this without a rule.

Two consequences for the implementation:

- **The octave count is per field, not global.** `Field::Fbm` already carries
  it per node; a single configuration value shared by every scale is an
  artificial coupling that forces the shortest field to run the longest field's
  ladder.
- **Nyquist is a validation bound, not the mechanism that picks the counts.**
  Deriving each count by truncating at the limit would make it a step function
  of a float, so retuning a wavelength by a tenth of a mile would silently flip
  a count and move every value in the world — the knife-edge threshold section
  25.6 warns about. Configuration states the counts; validation rejects a ladder
  that reaches below the limit; the limit is derived from `APOTHEM_MILES` rather
  than written as a literal.

### 9.4 Displace the sampling position by a seed-derived offset

Every noise lattice has its origin at the coordinate origin, so without an
offset the world origin is a lattice point of *every* scale at once and every
field is exactly zero there. Simplex noise has its steepest gradient at a
lattice point, so the region around the origin is measurably steeper than the
rest of the world — a permanent, visible anomaly at the one coordinate every
player frame is expressed against, every worked example uses, and every
diagnostic render defaults to. It is the same objection section 11.2 raises to
the anchor lattice and section 33.4 raises to region-owned terrain: no place may
be special because of how the implementation addresses it.

The fix is a translation of the sample position. It must be:

- **derived from the seed**, so two worlds do not share the anomaly's new
  location;
- **applied per domain**, so the scales do not all land on their own lattice
  points at some *other* single coordinate — moving the defect is not fixing
  it;
- **applied above the fbm rather than inside the leaf.** The fbm scales the
  sample position by the octave frequency before the leaf divides by the
  wavelength, so an offset folded into the leaf is the same fraction of a cell
  at every octave: at the origin every octave would then sample the same cell,
  with the same gradients, and their slopes would add constructively. That is
  the same defect with a smaller coefficient.
- **scaled by the wavelength**, so it is irrational-looking relative to that
  field's lattice instead of a round number of miles that some other wavelength
  divides, and **kept clear of the cell corners** — a hash is free to come back
  near zero, and a fix that works for most seeds is not a fix.

A single rendered window does not demonstrate any of this. At one seed the
origin is a bright dot among a handful of others. Only a measurement pooled over
many seeds separates it from terrain, because ordinary terrain is uncorrelated
between worlds and cancels, leaving whatever is a function of position relative
to the centre.

---

## 10. Multi-Scale Geography

A single noise frequency looks synthetic. WGVB combines several spatial scales.

```text
elevation =
    macro_continentalness
  + regional_uplift
  + ridge_structure
  + local_relief
  + fine_detail
```

Approximate starting wavelengths. One hex of wavelength means 6 miles of center-to-center distance; it does not refer to edge length, point-to-point width, or area.

| Field | Approximate wavelength | Approximate distance |
|---|---:|---:|
| Continentalness | 500–2,000 hexes | 3,000–12,000 miles |
| Macro uplift | 150–600 hexes | 900–3,600 miles |
| Regional relief | 40–200 hexes | 240–1,200 miles |
| Hills | 8–40 hexes | 48–240 miles |
| Local detail | 3–12 hexes | 18–72 miles |

These are starting values, not requirements. The exact constants live in the configuration structure, not scattered through the implementation.

The wavelengths above are each scale's *base*. Each is the top of an fbm ladder that descends by the lacunarity, so the octave count decides how far into the next scale's band a field reaches, and section 9.3 bounds how far it may. Once that bound truncates the shortest ladder hardest, the fields stop being strictly ordered by how fast they vary even though their base wavelengths still are; that is a consequence of the bound and not a defect to design around.

---

## 11. Hierarchical Regions

Continuous noise is supplemented by deterministic regional influences.

```text
macro region
    |
    +-- region
            |
            +-- chunk
                    |
                    +-- tile
```

Example sizes at the 3-mile apothem:

| Level | Axial interval | Physical interval |
|---|---:|---:|
| Macro region | 512 hexes | 3,072 miles |
| Region | 128 hexes | 768 miles |
| Chunk | 32 hexes | 192 miles |

The intervals describe spacing between anchors along either axial basis direction. Regions and chunks produced by independent division of `q` and `r` are parallelograms in world space, not regular hexagons with the listed physical interval as a diameter.

### 11.1 Why regions exist

Regions create persistent geographic character. A region deterministically derives parameters such as:

- average uplift,
- climate bias,
- roughness,
- ridge orientation,
- wet/dry tendency,
- volcanic tendency,
- terrain variation.

This gives large areas identity without storing anything.

### 11.2 No hard boundaries

A tile must not inherit parameters from exactly one region. That creates visible seams.

Blend regional parameters from neighboring anchors:

```text
value = w00*region00 + w10*region10 + w01*region01 + w11*region11
```

For hex-oriented or Voronoi-inspired schemes, blending may use another fixed local neighborhood.

WGVB takes the hex-oriented option, because the four-corner form makes an artifact that is easy to miss in a unit test and obvious in a wide render. The axial basis vectors are 60 degrees apart and equal in length, so anchors at `(i * size, j * size)` form a *triangular* lattice in world space and each cell is two equilateral triangles. Interpolating bilinearly across the cell privileges the cell's long diagonal, and a field built that way draws the lattice as rows of aligned lozenges. Interpolating across the containing triangle has no preferred diagonal and carries the lattice's own six-fold symmetry.

Barycentric coordinates on that triangle are already a partition of unity. Squaring each and renormalizing keeps the sum at one and makes a corner's weight and its first derivative vanish as the corner leaves the neighborhood, so a triangle edge — and a cell boundary, which is one — is a smooth join rather than a crease. Cubing joins smoothly too and looks worse: each anchor acquires a plateau and the plateaus meet along the hexagonal boundaries of the lattice's Voronoi cells, which is the lattice made visible by another route.

> Crossing a chunk or region boundary must not introduce a discontinuity merely because the addressing region changed.

---

## 12. Region Anchors

A lattice of deterministic anchors is the straightforward implementation. For each regional grid coordinate `(region_q, region_r)`, derive stable attributes by coordinate hashing:

```rust
#[derive(Debug, Clone, Copy)]
pub struct RegionParams {
    pub elevation_bias: f64,
    pub moisture_bias: f64,
    pub heat_bias: f64,
    pub roughness: f64,
    pub ridge_angle: f64,
}
```

`ridge_angle` is stored and consumed as a **unit direction vector**
`(cos, sin)`, not as an angle in radians, so that no trigonometric function
appears in the generation path. See section 25.2. Derive the vector by hashing
two values and normalizing, or by hashing a point and rejecting until it lands in
the unit disc — both use only multiply, add, and `sqrt`. The implementation names
the field `ridge` for that reason: a field called `ridge_angle` holding a vector
invites someone to put an angle in it.

A ridge orientation is a **line, not an arrow**: `v` and `-v` name the same
orientation, and consumers must take `|dot|` rather than `dot`. This is not a
detail — orientations cannot be averaged as vectors. Two anchors whose ridges run
the same way but hashed to opposite arrows would blend to nothing, and a tile
between them would get an orientation unrelated to either. Blend the doubled-angle
form `(x^2 - y^2, 2xy)` instead, in which `v` and `-v` are identical, and recover
the orientation with the half-angle identities `cos t = sqrt((1 + cos 2t) / 2)`
and `sin t = sqrt((1 - cos 2t) / 2)`, taking the sign of `sin t` from `sin 2t`.
Every step of that is multiply, add, divide, and `sqrt`. Where the blended
doubled-angle vector cancels to nothing — two anchor ridges at right angles — no
orientation exists; fall back deterministically, as the noise gradients do.

When generating a tile, evaluate the nearest relevant anchors and smoothly interpolate their contribution.

Region parameters are normalized *biases* in `[-1, +1]`, not quantities. How much
uplift an elevation bias is worth, or how many degrees a heat bias moves a tile,
belongs to the phase that consumes it — which is what lets elevation, climate,
and terrain be tuned independently without redefining what a region is.

---

## 13. Domain Warping

Straight noise reveals its mathematical origin. Domain warping is part of the recommended implementation.

Instead of:

```text
elevation = noise(x, y)
```

evaluate:

```text
wx = x + warp_x(x, y) * strength
wy = y + warp_y(x, y) * strength

elevation = noise(wx, wy)
```

This produces more irregular coastlines, mountain belts, climatic boundaries, and regional forms.

Warp fields must themselves be deterministic and continuous. Use low-frequency warps for large geography and weaker high-frequency warps for local irregularity.

---

## 14. Elevation

Elevation is the primary physical field, represented as a normalized `f64` scalar:

```text
-1.0 deep ocean
 0.0 sea level
+1.0 extreme highland
```

Values outside that range may be clamped.

Do not normalize elevation using the minimum and maximum of a finite generated map. That reintroduces a dependence on map bounds. Field composition and thresholds must be stable globally.

### 14.1 Elevation Classification

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum Elevation {
    DeepWater = 0,
    ShallowWater = 1,
    Lowland = 2,
    Upland = 3,
    Highland = 4,
    Mountain = 5,
}
```

> **Pin every discriminant explicitly.** These values are persisted and appear
> in cached tiles. Reordering variants silently changes stored data in exactly
> the way reordering a Go `iota` block does; writing `= 0`, `= 1`, … makes the
> hazard visible in review.

Rust enums also make classification *total*: a `match` that forgets a band is a
compile error, which Go's integer constants cannot provide.

The exact names are game-facing decisions and do not constrain the internal scalar field.

---

## 15. Water and Land

Land versus ocean water is determined by comparing elevation against a fixed sea-level threshold:

```text
elevation <= 0 -> ocean water
elevation >  0 -> potential land
```

Sea level may be configurable.

Deterministic basin fields may classify some potential-land tiles as lakes or inland seas if the coherence requirements in section 17 are met. Otherwise the first implementation retains basin geography without inland-water classification. Both approaches must use bounded local sampling, never global connectivity or flood fill.

The first implementation took the second path. See the decision record in section 17.1.

If a target land fraction is desired, tune the continentalness distribution and sea-level threshold statistically. Do not calculate sea level from a finite sample at runtime.

---

## 16. Climate

Climate is computed from at least temperature, moisture, and elevation:

```text
temperature = broad_heat_field + regional_heat_bias - elevation_cooling
moisture    = broad_moisture_field + regional_moisture_bias + local_variation
```

Because the wrapped world has no inherent equator, do not assume `r == 0` is a planetary equator unless that becomes an explicit world rule. The first version uses procedural broad heat zones rather than global latitude.

> If latitude is added later, express its falloff as a polynomial, not a cosine.
> See section 25.2.

### 16.1 Climate classification

Climate retains independent heat and moisture classifications. Do not use a single enum mixing values such as cold, arid, and humid; those properties are not mutually exclusive.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum HeatBand {
    Polar = 0,
    Cold = 1,
    Temperate = 2,
    Warm = 3,
    Hot = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum MoistureBand {
    Arid = 0,
    Dry = 1,
    Moderate = 2,
    Humid = 3,
    Saturated = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Climate {
    pub heat: HeatBand,
    pub moisture: MoistureBand,
}
```

Bands and thresholds may be tuned, but the two-axis representation is part of the public model. `Tile` also retains the normalized heat and moisture values the bands were classified from.

---

## 17. Terrain

Terrain is a classification derived from physical fields, never an independent random lookup.

```text
terrain = f(
    elevation,
    slope-or-relief,
    temperature,
    moisture,
    inland-water influence,
    volcanic tendency,
    regional character,
    local variation,
)
```

The initial vocabulary is broad enough to produce a varied fantasy map without requiring every distinction at once:

| Family | Suggested terrain types | Typical evidence |
|---|---|---|
| Ocean | deep ocean, ocean, shallow sea, coastal water | Elevation below sea level, depth, adjacency to land |
| Inland water | inland sea, lake | Basin fields, basin scale, depth, low local relief |
| Frozen | glacial ice, tundra | Low temperature, with elevation and moisture distinguishing persistent ice from tundra |
| Wetland | marsh, swamp, bog | Saturated moisture, low elevation, low relief, temperature |
| Dry | desert, badlands, scrubland | Low moisture, heat, exposed relief, regional character |
| Open land | plains, grassland, steppe, savanna | Moderate moisture and temperature, regional variation |
| Forest | boreal forest, temperate forest, tropical rainforest or jungle | Sufficient moisture with the appropriate heat band |
| Elevated | hills, mountain, alpine terrain | Elevation, relief, slope, temperature |
| Volcanic | volcano, volcanic highland | Strong volcanic tendency with uplift and concentrated relief |
| Coastal land | coast | Land near sea level with an adjacent ocean-water tile |

These are primary game-facing classifications. Elevation, relief, and climate remain available so a game can render forested hills, glaciated mountains, or a volcanic island without a distinct terrain constant for every combination.

Order classification rules so exceptional terrain is not hidden by a broad biome rule:

```text
ocean and inland water
    -> glacial ice
    -> volcano
    -> mountain and alpine terrain
    -> wetland
    -> climate-driven land cover
```

Ocean water still comes from the primary elevation field and sea-level threshold. Make a best effort to derive coherent depressions from deterministic basin fields at broad, regional, and local scales. Basin influence is useful even when no water is assigned: dry endorheic regions such as the Great Basin are valid geographic results.

Classify a basin as a lake or inland sea only if bounded local generation can give neighboring water tiles coherent membership, surface elevation, depth, and shorelines. The lake/inland-sea distinction is then based on generated basin scale and depth, not global connectivity or flood fill. If those invariants cannot be achieved simply and deterministically, omit inland-water terrain from the first implementation rather than emitting inconsistent per-tile water. A generated inland sea is a very large basin lake, not water proven disconnected from every ocean in the wrapped world.

Marsh and swamp are distinguished primarily by climate and vegetation tendency: marshes favor open saturated lowlands, swamps favor warmer or forested saturated lowlands. Volcanoes are rare products of regional volcanic tendency, uplift, and local peak structure, not independent random tile assignments.

Terrain is an enum with pinned discriminants, on the same terms as section 14.1.

### 17.1 Decision record — inland water is omitted

**Phase 6 shipped basin geography and no inland water.** No world produced at
`ALGORITHM_VERSION = 5` contains a tile classified `Lake` or `InlandSea`. The
two variants stay in the vocabulary with their discriminants pinned, because
they are part of the persisted value space and the version that does emit them
must not renumber the twenty-one variants after them.

The paragraph above permits inland water only where bounded local generation
can give neighboring water tiles coherent membership, surface elevation, depth,
and shorelines, and requires omission rather than approximation otherwise. It
cannot, and the obstruction is structural:

- A lake has **one** surface elevation. Every tile of one lake must agree on
  it, or the water runs downhill inside itself and a tile's depth is not a
  depth.
- Which tiles are "one lake" is a **connected component** of the ground below
  that surface. Finding it is a traversal whose extent is the lake's, which is
  unbounded in principle — and sections 2.3 and 15 forbid global connectivity
  and flood fill outright.
- A *smooth* water-surface field, with a tile under water where its elevation
  falls below it, is bounded and deterministic and visibly wrong: the surface
  varies across the lake, so the lake is tilted, two hollows a few hexes apart
  have different water levels, and the shoreline is where two smooth fields
  happen to cross rather than a level line.
- Giving each addressing cell a lake with a hashed surface elevation
  reintroduces the hard region boundary of section 33.4, draws the lattice on
  the map, and puts water on hillsides wherever the cell's elevation does not
  match the ground.

What ships instead is the geography. Basin influence exists at broad, regional,
and local scales, blended with the region basin bias, and terrain reads it
through a *product* rather than a sum: terrain wetness is
`moisture + weight * basin * moisture`, so a basin makes a wet climate wetter
and a dry one drier, and a rise sheds water either way. That is this section's
own example — dry endorheic regions such as the Great Basin are valid
geographic results — arriving as a consequence of the composition rather than
as a special case. A wet basin reads as marsh, swamp, or bog. What is missing
is open water in the middle.

**Basin influence does not feed elevation, and must not.** A basin term inside
the elevation composite is the obvious way to make a depression *be* lower
ground, and it would move every tile in every world. The golden table in
`crates/wgvb/tests/golden.rs` is what enforces the placement: phase 6 added
three columns and moved none of the twelve that were already there.

---

## 18. Local Relief and Slope

Some terrain decisions require knowing whether a location is flat, hilly, or steep. Estimate slope by sampling elevation at the six neighboring hexes.

```rust
impl Generator {
    pub fn relief(&self, c: Coord) -> f64;
}
```

Because generation is deterministic and stateless, sampling neighbors creates no dependency problem.

Avoid recursive terrain classification:

```text
raw fields
    -> elevation scalar
    -> neighbor elevation samples
    -> derived relief
    -> climate
    -> terrain
```

**Do not call `tile()` from inside `tile()`.** Structure the internals as a
private `elevation_scalar(&self, c: Coord) -> f64` that both `tile` and `relief`
call, so the recursion cannot be written. Rust will not catch this for you.

The neighbor loop must accumulate in a fixed direction order, `0` through `5`. See section 25.3.

---

## 19. Public API

A minimal initial API for the `wgvb` crate:

```rust
pub type Seed = u64;
pub type Component = i16;

pub struct Coord { /* private fields; see section 4 */ }

pub struct Generator { /* immutable configuration */ }

impl Coord {
    pub fn new(q: i64, r: i64) -> Coord;
    pub const ORIGIN: Coord;
    pub fn q(self) -> Component;
    pub fn r(self) -> Component;
    pub fn s(self) -> Component;
    pub fn neighbor(self, direction: i32) -> Coord;
}

impl Generator {
    pub fn new(seed: Seed, config: Config) -> Result<Generator, ConfigError>;
    pub fn with_defaults(seed: Seed) -> Generator;

    pub fn tile(&self, c: Coord) -> Tile;
    pub fn elevation_at(&self, c: Coord) -> f64;
    pub fn relief(&self, c: Coord) -> f64;
    pub fn sample(&self, c: Coord) -> Sample;

    pub fn seed(&self) -> Seed;
    pub fn config(&self) -> &Config;
    pub fn config_fingerprint(&self) -> [u8; 32];
}
```

`Coord::new` is the entry point for unwrapped or intermediate coordinates and returns the canonical representative. `neighbor` normalizes before returning. Because `Coord` has no other constructor, `tile` has nothing to re-normalize — the type already guarantees canonical input.

`Generator::new` returns `Result` because configuration validation can fail; `with_defaults` cannot fail and is the ergonomic path for tests and the diagnostic harness.

Do not expose chunk generation as the primary abstraction. Applications ask directly for a tile.

### 19.1 Error model

The Go design left errors implicit. Make them a typed enum per crate, using `thiserror`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{field} must be finite, got {value}")]
    NotFinite { field: &'static str, value: f64 },
    #[error("{field} must be positive, got {value}")]
    NotPositive { field: &'static str, value: f64 },
    #[error("{field} must be in [{lo}, {hi}], got {value}")]
    OutOfRange { field: &'static str, value: f64, lo: f64, hi: f64 },
    #[error("{field} must be in 1..=16, got {octaves}")]
    OctaveCount { field: &'static str, octaves: u8 },
    #[error("{field} = {octaves} puts an octave below the Nyquist wavelength")]
    BelowNyquist { field: &'static str, octaves: u8, /* ... */ },
}
```

The database compatibility gates in section 27 get the same treatment, one variant per gate, so section 30.11 asserts on variants instead of matching error strings.

---

## 20. Batch API

Rendering and simulation request rectangular or hexagonal groups. Batch APIs reduce repeated setup.

```rust
impl Generator {
    /// Fills `out` with one tile per coordinate. Panics if lengths differ.
    pub fn tiles_into(&self, coords: &[Coord], out: &mut [Tile]);

    pub fn tiles(&self, coords: &[Coord]) -> Vec<Tile>;
    pub fn region(&self, center: Coord, radius: u32) -> Vec<Tile>;
}
```

Prefer the `_into` form in hot paths: `Tile` is `Copy`, so a caller can reuse one buffer across frames with no allocation.

Batch results must be bit-identical to individual `tile()` calls. Batch generation is an optimization only.

`rayon` may parallelize batch fills. This is safe *and* deterministic here for a
structural reason: every tile is a pure function of its own coordinate and
writes to its own slot, so work-stealing order cannot affect any value. Do not
introduce any batch operation that accumulates across tiles — a running sum, a
min/max, a histogram — because floating-point addition is not associative and
the result would depend on the split. Section 30.3 becomes near-trivially true
under this rule.

---

## 21. Configuration

Configuration is immutable after construction.

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub sea_level: f64,

    pub continental_wavelength_miles: f64,
    pub regional_wavelength_miles: f64,
    pub local_wavelength_miles: f64,

    pub warp_wavelength_miles: f64,
    pub warp_strength_miles: f64,

    pub region_size_hexes: u32,
    pub chunk_size_hexes: u32,
    // ...
}
```

Field names above are illustrative. The implemented configuration must include **every** weight, scale, threshold, and feature toggle that can alter generated output. Scale fields must state whether they are wavelengths or frequencies and in what unit — the names above use `_wavelength_miles` and `_hexes` suffixes for exactly this reason.

Validation must reject non-finite floats, non-positive sizes and scales, out-of-range normalized thresholds, and combinations that cannot be evaluated safely. It must also reject an fbm ladder whose octaves reach below the tile grid's Nyquist wavelength (section 9.3), because that combination is evaluable and wrong rather than unevaluable.

The complete effective configuration, including defaulted values, is authoritative database data for the single world. Creating a database writes that complete configuration before gameplay; reopening never silently substitutes current program defaults for missing stored values.

### 21.1 Serde rules that protect the world

Two `serde` attributes carry real weight here, and one is a trap:

- **`#[serde(deny_unknown_fields)]` is required.** An older binary reading a
  newer world file must *reject* it, not silently ignore the fields it does not
  understand. This is the same guarantee section 27 wants from the generator
  version gate, enforced one level down.
- **Never put `#[serde(default)]` on a field that affects generation.** A
  defaulted missing field is a changed world with an unchanged version number —
  precisely the failure the whole versioning scheme exists to prevent. If a new
  field must be added, that is an algorithm version change.
- `#[serde(default)]` is acceptable only on fields that cannot alter output,
  such as a human-readable world name.

### 21.2 Configuration fingerprint

Each algorithm version defines a canonical serialization of its effective configuration. Compute a stable fingerprint for cache identity and diagnostics:

```text
fingerprint = SHA-256( algorithm_version_le_bytes || canonical_config_bytes )
```

Rules:

- Serialize to **CBOR via `ciborium`**, not JSON. JSON invites whitespace,
  float-formatting, and key-ordering variance; CBOR from a struct is
  deterministic in field order.
- Hash `f64` values as `to_bits()`, never as a formatted string. Normalize
  `-0.0` to `0.0` before hashing, and reject `NaN` during validation so it can
  never reach the fingerprint.
- Do not derive the fingerprint from `Debug` output, `HashMap` iteration, or any
  serialization with unspecified field ordering.
- Do not use `std::collections::hash_map::DefaultHasher`. See section 25.4.

**The defaults are settled by this, not by intention.** Phase 7's remaining
tuning task was to fix the defaults an algorithm version ships with, and a
fingerprint over the complete effective configuration is what fixes them:
`crates/wgvb-store/tests/fingerprint.rs` carries the fingerprint of
`Config::default` as a written-down constant, so moving any default — a
wavelength, a weight, a threshold, an octave count — fails that test on the spot.
Updating the constant is the compatibility decision, and it belongs in a commit
message alongside the `ALGORITHM_VERSION` bump that goes with it.

The `ciborium` float encoding is worth knowing about here: it writes each float
in the shortest CBOR form that represents it exactly, so the encoding is a
dependency's policy rather than this crate's. That is still binary and still
exact — the rule that an `f64` is hashed as its bits is kept — and the written
constant is the tripwire if a release ever changes the policy.

---

## 22. Concurrency

A configured `Generator` is safe for concurrent read-only use, and in Rust this is checked by the compiler rather than promised in a comment.

```rust
let g = Generator::with_defaults(seed);
std::thread::scope(|s| {
    s.spawn(|| g.tile(a));
    s.spawn(|| g.tile(b));
    s.spawn(|| g.tile(c));
});
```

This compiles only if `Generator: Sync`, which holds automatically as long as it contains no interior mutability. Assert it so a future field cannot quietly remove the property:

```rust
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Generator>();
    assert_send_sync::<Config>();
    assert_send_sync::<Tile>();
};
```

Section 26's rule that caches must be concurrency-safe, optional, or outside the core is likewise no longer a review checklist: a cache with interior mutability *appears in the type*, and adding one to `Generator` breaks the assertion above at compile time.

The crate sets `unsafe_code = "forbid"` at the workspace level. There is no reason for `unsafe` in a pure-function world generator, and forbidding it removes the only way to defeat these guarantees.

---

## 23. Chunks

Chunks are useful for callers and caches but do not define geography.

Recommended starting size: `32 x 32` axial-addressed cells.

The simplest implementation divides `q` and `r` independently:

```rust
let chunk_q = (q as i64).div_euclid(CHUNK_SIZE);
let chunk_r = (r as i64).div_euclid(CHUNK_SIZE);
```

With chunk size `32`:

```text
q =   0 -> chunk  0
q =  31 -> chunk  0
q =  32 -> chunk  1

q =  -1 -> chunk -1
q = -32 -> chunk -1
q = -33 -> chunk -2
```

Implement and test this explicitly.

---

## 24. Negative Coordinates

Negative coordinates are first-class. All helper math must behave correctly across the origin: floor division, modulo, region lookup, interpolation cells, chunk boundaries.

**The Go design's hand-written `floorDiv` and `floorMod` helpers are not needed.**
`i64::div_euclid` and `i64::rem_euclid` are in `std`, and for a **positive**
divisor — which every divisor here is (chunk size, region size, `6`) — Euclidean
division is exactly floor division and Euclidean remainder is exactly
mathematical modulo.

Two things this does not excuse:

- Keep the exhaustive around-zero tests. The helpers went away; the boundary
  cases did not.
- Euclidean and floor division *differ* for negative divisors. If a negative
  divisor ever appears, this equivalence stops holding. Assert positivity at the
  configuration boundary so it cannot.

---

## 25. Floating-Point Stability

This section replaces the Go design's advisory guidance with rules. Rust can
deliver genuine bit-for-bit reproducibility across targets, but only if the
generation path stays inside the operations that IEEE-754 specifies exactly.

### 25.1 Rust does not contract into FMA

The Go specification *permits* an implementation to fuse `x*y + z` into a single
fused multiply-add, and noise code is almost entirely multiply-adds. On arm64 Go
fuses; on amd64 targets without FMA it may not — so the same tile can differ in
its low bits between a developer laptop and a server. Go's workaround is writing
`float64(x*y) + z` at every such site, and missing one is a silent
cross-platform world divergence.

Rust never contracts implicitly. `a * b + c` is a multiply that rounds, then an
add that rounds, on every target. `f64::mul_add` is the explicit opt-in.

**Rule: do not call `f64::mul_add` anywhere in the generation path.** It is a
different function with different results, and using it in some places and not
others is how the Go hazard gets reintroduced by hand.

### 25.2 Restrict the generation path to exact operations

> **The generation path uses only `+`, `-`, `*`, `/`, `sqrt`, `floor`, `abs`,
> `min`, `max`, and comparisons on `f64`. No transcendental functions.**

Rust's `sin`, `cos`, `exp`, `powf`, `ln`, and friends route to the platform
libm, which is **not** bit-identical across operating systems and architectures.
This is the one place Rust is weaker than Go, whose `math` package is largely
portable Go code.

The restriction is not a hardship. Every operation the design actually needs
satisfies it:

- gradient, simplex, and value noise: multiply, add, floor, and table lookup;
- quintic and smoothstep interpolation: polynomials;
- domain warping (section 13): addition of field outputs;
- vector normalization: `sqrt`;
- the multi-scale sum (section 10): weighted addition;
- ridge orientation (section 12): a stored unit vector, not an angle;
- climate falloff (section 16): a polynomial, not a cosine.

If a future field genuinely requires a transcendental, the alternative is the
[`libm`](https://crates.io/crates/libm) crate — a pure-Rust MUSL port that is
bit-identical everywhere — pinned exactly. Do not reach for `std` and hope.

Also note `powi` is fine (it is repeated multiplication) but its association
order is compiler-defined; write `x*x*x` explicitly where the value matters.

### 25.3 Fixed accumulation order

Never accumulate `f64` in an order that can vary. Floating-point addition is not
associative, so a different order is a different number.

- Iterate the six directions in fixed order `0..6`, never over a `HashSet`.
- Iterate octaves from coarsest to finest, always.
- Do not `par_iter().sum()` over anything in the generation path (section 20).
- Where a set of coordinates must be reduced, sort it first. `Coord` is four
  bytes and derives `Ord`; a sorted `Vec<Coord>` beats both `HashSet` and
  `BTreeSet` here.

Rust's `HashMap` and `HashSet` are randomized per process exactly as Go's maps
are. The Go design's "avoid reducing over Go maps" warning applies unchanged.

### 25.4 `DefaultHasher` is version-unstable

`std::collections::hash_map::DefaultHasher` is documented as having an
unspecified internal algorithm that **may change between Rust releases**. It is
deterministic within one toolchain, which is exactly what makes it dangerous: it
will pass every test today and silently invalidate every persisted world and
every cache the day the toolchain is upgraded.

**Never use it for a fingerprint, a cache key that outlives the process, or
anything in the generation path.** Section 21.2 specifies SHA-256 over canonical
CBOR; section 8 specifies a mixer written out in this crate. Both are stable
because we own them.

### 25.5 Float-to-integer casts

Rust's `as` cast from float to integer *saturates* (and maps `NaN` to zero)
deterministically. Go leaves out-of-range float-to-integer conversion
implementation-specific. This is a small Rust advantage, but saturation is
rarely the behavior you actually want in a classifier — prefer explicit
`clamp` followed by the cast, so the intended range is visible at the call site.

### 25.6 Thresholds

Classification thresholds must not be pathologically sensitive to microscopic
differences. Golden tests (section 30.9) verify representative coordinates
bit-exactly; distribution tests (section 30.8) verify that thresholds are not
sitting on a knife edge.

### 25.7 Build settings

Do not add any compiler or profile flag that relaxes floating-point semantics.
The workspace `[profile.release]` deliberately contains no such flag, and the
crate does not use nightly fast-math intrinsics. `-C target-cpu=native` is
acceptable for local benchmarking and must never be used for a build whose
output is compared against goldens.

---

## 26. Caching

The generator must work correctly with no cache. Applications may cache individual tiles, elevation samples, region parameter blocks, or rendered chunks.

Region parameter caching is the likeliest first optimization, since many nearby tiles reuse the same anchors:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionCoord {
    pub q: i32,
    pub r: i32,
}
```

Introduce caching only after profiling, and note that at Rust's throughput
(section 31) the profile may well say *don't*. If it is needed, a small
direct-mapped array — `[Option<(RegionCoord, RegionParams)>; 64]` indexed by a
few hash bits — beats a `HashMap` for this access pattern and avoids hashing
entirely. Either way it lives behind `&self`, so it needs interior mutability
and therefore belongs **outside** `Generator` per section 22.

Correctness must never depend on cache history.

---

## 27. Persistence and Versioning

Generated terrain is a function of:

```text
algorithm version + seed + configuration + coordinates
```

A WGVB database contains exactly one world and everything needed to reproduce that world's generated baseline.

```rust
pub const ALGORITHM_VERSION: u32 = 1;
```

The database persists, in singleton world metadata:

```text
world seed
algorithm version
complete effective generator configuration
configuration fingerprint
```

Changing noise formulas, thresholds, or hash domains changes existing worlds. Treat such changes as generation-version changes unless compatibility is intentionally preserved.

### 27.1 Store: SQLite via `rusqlite`

Use [`rusqlite`](https://crates.io/crates/rusqlite) with the `bundled` feature,
which compiles SQLite from source so there is no system dependency and no
version skew between developer machines. Do not add an ORM or a query builder.

`rusqlite` is the direct analog of the Go design's `zombiezen.com/go/sqlite`:
a thin, direct API rather than a generic database abstraction layer.

Every database sets:

```rust
pub const APPLICATION_ID: i32 = 0x5747_5642; // ASCII "WGVB"; decimal 1464292930
```

Note this differs from WGVA's `0x57475641`. A WGVA file must be rejected by a WGVB binary at the first gate, and vice versa.

### 27.2 Coordinate-keyed tables are `WITHOUT ROWID`

Every table keyed by canonical `(q, r)` — overlays and any cache — is declared:

```sql
CREATE TABLE overlay_settlement (
    q INTEGER NOT NULL,
    r INTEGER NOT NULL,
    -- ...
    PRIMARY KEY (q, r)
) WITHOUT ROWID;
```

This matters more than it looks. A `WITHOUT ROWID` table *is* a B-tree keyed by
the composite primary key, so loading every overlay in a viewport or a chunk is
one ordered range scan rather than a rowid lookup per row through a secondary
index. It gives the coordinate locality that would otherwise be the main reason
to reach for a dedicated key-value store. See appendix C.

### 27.3 No foreign keys to generated data

The Go design required foreign keys for relational integrity. **Drop that
requirement here.** In a single-world database whose only relational structure
is coordinate-keyed sparse overlays, the only plausible foreign-key target is a
tiles table that this very document calls a discardable cache. Authoritative
player state must never have a referential dependency on regenerable data —
dropping the cache would then require dropping the player's settlements.

Enable `PRAGMA foreign_keys = ON` on every connection anyway, so that any
constraint deliberately added later between two authoritative tables is actually
enforced.

### 27.4 Migrations

There is no `sqlitemigration` equivalent in Rust, and none is needed. The
`user_version` ladder is roughly forty lines and is what that library does:

```rust
const MIGRATIONS: &[&str] = &[
    // index 0 migrates schema version 0 -> 1
    include_str!("../migrations/0001_initial.sql"),
];
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;
```

Apply pending migrations in order inside one transaction each, setting
`PRAGMA user_version` as the final statement of each step. **Never edit a
released migration.** Owning this code is preferable to a dependency for the
same reason as the noise (section 9.2): the migration ladder is part of the file
format, and the file format must not move because a dependency did.

### 27.5 Opening gates

```text
1. Verify PRAGMA application_id is WGVB (an empty database is initialized instead).
2. Read PRAGMA user_version and reject a schema newer than the binary.
3. Apply supported ordered schema migrations.
4. Read and validate the singleton world metadata and complete configuration.
5. Reject a generator version the binary cannot reproduce.
6. Permit normal reads and writes.
```

Each gate is one variant of a typed error (section 19.1):

```rust
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("not a WGVB database: application_id is {found:#010x}, expected {expected:#010x}")]
    WrongApplicationId { found: i32, expected: i32 },
    #[error("schema version {found} is newer than this binary supports ({supported})")]
    SchemaTooNew { found: u32, supported: u32 },
    #[error("world was generated by algorithm version {0}, which this binary cannot reproduce")]
    UnsupportedGeneratorVersion(u32),
    #[error("world metadata is missing or malformed: {0}")]
    MalformedMetadata(String),
    #[error("stored configuration does not match its fingerprint")]
    FingerprintMismatch,
    #[error(transparent)]
    Config(#[from] wgvb::ConfigError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}
```

No gate may perform an application write before it passes. Older generator versions are rejected unless the binary deliberately retains their implementations. **A generator incompatibility must never be handled by silently regenerating the world with current rules.**

### 27.6 What is authoritative and what is not

Persist mutable game and player state as sparse overlays keyed by canonical `(q, r)`. No `world_id` column: one database holds one world.

Generated tiles, generated chunks, and PNG files are reproducible caches, not authoritative records, and may be discarded at any time. Any cache entry must carry — and be validated against — the configuration fingerprint and, for rendered output, the palette/render version.

Given the throughput in section 31, **do not build the tile cache in the first
implementation.** Measure first. A cache that is never faster than regeneration
is pure liability: a fingerprint to validate and a staleness bug to hit.

**Measured, phase 7: no tile cache.** `crates/wgvb/tests/bench.rs` puts tile
generation at roughly 130,000 tiles per second per core on an M-series laptop —
about 7.4 microseconds a tile, near-identical for single tiles, chunk fills, and
a radius-32 region, which is what a pure function with no shared state should
look like. A 400x300 diagnostic window is therefore around 0.9 seconds of one
core, and the scrolling steps in section 29.1 move by half a window.

That is above the floor and below the hope; see section 31. It is still not an
argument for the cache, for two reasons. A cache would have to be validated
against the configuration fingerprint on every hit, and the thing it would save
is already embarrassingly parallel: section 22 permits a `rayon` batch fill, and
eight cores is a larger and simpler win than any cache with a coherency story.
Revisit this when a profile shows the same coordinates being generated
repeatedly, which a bounded viewport render does not do.

### 27.7 Connection handling

`rusqlite::Connection` is `Send` but not `Sync`. Either give each thread its own
connection or hold one behind a `Mutex`. Do not build a pool before there is a
measured reason for one; the read volume against authoritative data is small,
and the generator — the part that is actually hot — touches no database at all.

---

## 28. Workspace Layout

```text
wgvb/
    Cargo.toml                  workspace root; all dependency versions pinned here
    DESIGN.md
    CLAUDE.md
    crates/
        wgvb/                   core generator
            src/
                lib.rs          Component, WORLD_RADIUS, ALGORITHM_VERSION
                coord.rs        Coord, normalization, neighbors, rotation, chunks
                hash.rs         domain constants, mixers
                noise.rs        owned noise implementation (private)
                field.rs        Field enum, fbm, warp
                config.rs       Config, validation, fingerprint
                generator.rs    Generator, tile, sample, batch
                elevation.rs
                climate.rs
                terrain.rs
                relief.rs
                region.rs
        wgvb-store/             single-world SQLite persistence
            migrations/
                0001_initial.sql
            src/
                lib.rs          APPLICATION_ID, OpenError, re-exports
                schema.rs       the user_version migration ladder
                world.rs        World; creation and the opening gates
                overlay.rs      sparse coordinate-keyed player overlays
                fingerprint.rs  canonical CBOR and SHA-256
        wgvb-render/            bounded viewport rendering; owns the hexx dependency
            src/
                lib.rs
                palette.rs      the diagnostic ramp, climate table, terrain list
                overlay.rs      Overlays; what the player knows
                frame.rs        PlayerFrame; player-relative <-> canonical
        wgvb-map/               diagnostic and player-facing CLI
            src/main.rs
        wgvb-serve/             local web viewer for one world, or any seed
            src/
                lib.rs          routes and the pure request-to-response function
                view.rs         View; the URL is the whole state
                source.rs       Source; a stored world, or the defaults
                page.rs         the one HTML page
                reply.rs        routing, rendering, and the error mapping
                main.rs
```

Rust module privacy is finer-grained than Go's package boundary, so there is no
`internal/` directory: a module is private unless declared `pub`, and
`pub(crate)` covers the rest.

**Where the player frame lives.** `Coord::rotate` belongs in the core crate: it
is a pure coordinate operation, and the rotation is required there anyway to
generate `MIRROR_CENTERS`. `PlayerFrame` belongs in `wgvb-render`, because a frame
exists to present the world to somebody and that is where the flat-top and
screen-axis offsets already compose. `wgvb-store` persists only the three scalars —
origin `q`, origin `r`, rotation — so it needs no dependency on the frame type.
`wgvb-render` and `wgvb-store` are siblings, so this placement is also what keeps a
player concept out of `wgvb` itself; see appendix A.

**The workspace is doing real work here.** The Go design could only *advise*
"do not introduce persistence into the core package". Here it is a fact of the
dependency graph: `wgvb` does not depend on `wgvb-store`, `rusqlite`, `png`, or
`hexx`, so persistence and rendering cannot leak into the generator without an
edit to `crates/wgvb/Cargo.toml` that a reviewer will see.

Dependencies flow one direction only:

```text
wgvb-map    ->  wgvb-render  ->  wgvb
    |               |
    +-> wgvb-store ----------->  wgvb

wgvb-serve  ->  wgvb-render  ->  wgvb
    |               |
    +-> wgvb-store ----------->  wgvb
```

`wgvb-serve` gained its `wgvb-store` edge with `--db`, which section 29.1 had
planned for from the start. It points the same way as every other edge here:
toward the core, never away from it, and the core still depends on exactly
`serde` and `thiserror`.

`wgvb-serve` is a separate crate rather than a mode of `wgvb-map`. A server
drags in an HTTP stack, and possibly an async runtime, and the CLI has no use
for either; keeping them apart keeps `wgvb-map --help` honest and keeps the
diagnostic CLI buildable without a web server in the tree. **The core crate
gains nothing from either** — it still depends on exactly `serde` and
`thiserror`.

---

## 29. Diagnostic Renderer

`wgvb-map` renders a bounded image of an arbitrary window into the effectively unbounded wrapped world. Use `clap`'s derive API for the command surface.

```bash
wgvb-map \
    --db world.wgvb \
    --q -200 \
    --r -150 \
    --cols 400 \
    --rows 300 \
    --hex-radius 8 \
    --layer terrain \
    --out map.png
```

The database supplies the seed, algorithm version, and effective configuration. If this command creates a new database, creation writes all of those values before rendering; it must never override them when opening an existing database. `cols` and `rows` are tile counts; `hex-radius` is a pixel dimension.

Before persistence exists, the same rendering code may be driven by an explicitly constructed in-memory generator as a development harness. Such output is diagnostic and does not represent a saved world. Player-facing rendering always loads its effective configuration from the database.

Useful layers:

```text
elevation, continentalness, temperature, moisture,
relief, climate, basin, volcanic, terrain, region influence
```

This tool is essential for tuning. Build it early (phase 2), because visual quality cannot be established by unit tests.

Rendering notes:

- **The renderer is where north exists.** It draws the admin frame: flat-top,
  image `y` downward, no rotation, which puts absolute direction `2` at the top
  of the image. Reading the compass clockwise from there — N, NE, SE, S, SW, NW
  — walks the direction index backwards, `2, 1, 0, 5, 4, 3`, because index order
  is counter-clockwise as a viewer sees it. Appendix A's *Rotation senses* is
  the whole story and the one place to change it; a test in `wgvb-render` pins
  the mapping, because a mirrored layout would keep every golden pixel passing
  while sending every printed heading the wrong way.
- Use `hexx` layouts and polygon corners for pixel geometry, and the `png` crate
  for encoding. Never let renderer pixel coordinates feed back into generation.
- Render coordinates in a stable sorted order. Overlapping edges and labels make
  output order-dependent otherwise.
- Keep generated terrain separate from player overlays — discoveries, fog of
  war, settlements, labels, annotations — and compose the layers at render time.
- **Golden-compare decoded RGBA buffers, not PNG file bytes.** The `png` crate's
  default filter strategy and compression level can change between versions,
  producing different bytes for identical images. Comparing pixels tests what we
  actually care about.

### 29.1 Web viewer

`wgvb-map` renders one window to one file, so looking at a world means running
it, opening the PNG, working out the next window's coordinates by hand, and
running it again. That is fine for recording a golden image and hopeless for
finding out what a seed looks like, which is the thing the renderer exists for.
`wgvb-serve` is the same renderer behind three URLs:

```text
GET /seed/{seed}                        the viewer page, centered on the origin
GET /seed/{seed}?q=-87&r=6543           the viewer page, centered on (-87, 6543)
GET /seed/{seed}/map.png?q=..&r=..      the rendered window itself
```

**This is deliberately not a single-page application.** No client-side panning,
no canvas, no script needed to move the view; page refresh is fine. Every state
the viewer can be in is a URL, which also makes "look at this" a link somebody
can paste into an issue, and makes the round trip testable: the links the page
emits must parse back to the state that produced them.

`{seed}` is sixteen hexadecimal digits, case-insensitive, with no `0x`, because
that is how a seed is written everywhere else. Note the spelling disagreement
this creates with `wgvb-map --seed`, which takes decimal because that is
`clap`'s default for a `u64`. Teaching the CLI the hex spelling too is worth
doing and is not done here.

Coordinates in the URL are **canonical**, not player-relative. This viewer has
no player, a canonical link means the same tile to everybody who opens it, and
it is the same number `wgvb-map --q --r` takes, so a window moves between the
two tools without a conversion. A frame-relative viewer would have to say so in
the URL — `?pq=&pr=` — rather than leaving two readings of the same link.

Defaults, all overridable by query string and all clamped: `q` and `r` at the
origin, `cols` 61, `rows` 45, `hex-radius` 10 pixels, `layer` `elevation`.

**Odd tile counts matter more than they look.** `Viewport::new` takes the
window's *first* tile, not its center, so a viewer has to convert; with an even
count there is no center cell, the conversion has to round, and a rounded
conversion makes a scroll step that returns to where it started stop returning
to where it started. The conversion is `Viewport::centered_on`, in
`wgvb-render`, because it is offset-scheme arithmetic and `wgvb-render` owns the
offset scheme. It rejects an even count rather than rounding one.

**Scroll distances are whole hexes**, counted in tiles and never in pixels, so
a step means the same thing at every zoom and a step followed by its opposite
returns to exactly the coordinate it started from. North and south move
`rows / 2` hexes; the four diagonals move `cols / 2`. Both are integer division
of an odd count. The new center is `n` whole steps of a vector from
`DIRECTIONS`, and there is no offset-coordinate arithmetic in the server at all:
`Coord::new` normalizes, so scrolling off an edge of the canonical hexagon wraps
to the opposite edge with no special case. It will look like a seam, because it
is one — the accepted world-warp seam of section 7.1. The viewer does not
pretend otherwise.

The six controls are the compass walk of appendix A in the admin frame, which
the renderer draws without rotation: N is absolute direction 2, and the walk
clockwise from there *decreases* the index, `2, 1, 0, 5, 4, 3`. When a player
frame arrives, north becomes absolute direction `k` and the same walk applies.
No rotation reaches the generator.

Bounded, like everything else that renders, and a server is an easier place to
forget it than a CLI:

- `cols`, `rows`, and `hex-radius` are clamped before a `Viewport` is built. The
  clamps are odd numbers, so a clamped request still has a center cell.
- `RenderError::TooLarge` becomes a 400 with a readable message, never a 500.
  Nothing a caller can type is the server's fault.
- A request costs `cols * rows` generator calls, and seven times that for the
  relief layer. The clamp is the answer, not a cache: section 26 asks for a
  profile before a cache.
- **Bind to `127.0.0.1` by default.** This is a diagnostic tool with no
  authentication and an endpoint whose cost the caller chooses. A `--host` flag
  exists; the default must not be `0.0.0.0`.
- **The page names the tile at its center**, not only the coordinate: the
  terrain, the elevation band, the two climate bands, and the scalars they
  were classified from. A link to a window is otherwise a link to a picture,
  and a reader has to count swatches against the key to find out what they are
  looking at. It costs one tile against the `cols * rows` the image beside it
  costs.
- The PNG is a pure function of seed, center, window, layer, `ALGORITHM_VERSION`
  and `RENDER_VERSION`, so it carries a strong `ETag` built from exactly those.
  That is the validity rule section 26 states for any cached render, and it
  costs one header. The configuration fingerprint of section 21.2 joins that
  list the moment a configuration can vary.
- **The page is a second representation and needs a revision of its own.** The
  two versions above describe the world and the pixels and say nothing about
  the HTML, so a release that changes the markup alone emits the same strong
  tag for different bytes and a browser goes on showing the old page — which
  is the one thing a strong validator exists to prevent. `PAGE_VERSION` in
  `wgvb-serve` is that revision; it appears in the page tag and deliberately
  not in the image tag, so changing the markup does not invalidate a cached
  PNG.

The HTTP stack is `tiny_http` and a fixed worker pool rather than `axum` and
`tokio`. The work is CPU-bound rendering with no IO to overlap, so an async
runtime would buy nothing and cost about eighty crates against about five.

Same standing as `wgvb-map`, and the same two modes. Without `--db` the
generator is constructed in memory from the seed in the route and the default
configuration, so **output is diagnostic and does not represent a saved world**,
and the page says so; every seed is servable, because the route is where the
world comes from.

With `--db` the database supplies the seed, the algorithm version, and the
complete effective configuration, and **the seed in the route is a check against
the stored one rather than the source of it** — another seed is a 404 naming the
one this server holds. The page says that too, in the opposite words. Overlays
are read fresh from the database on every request, so exploring a world and
refreshing shows the exploration.

`World` is `Send` and not `Sync`, so section 27.7's arrangement here is one
connection per worker thread, opened **before the port is bound**: a database
that fails an opening gate is a server that does not start rather than a server
that answers every request with a 500. The server opens and never creates —
`wgvb-map --db` creates a world, because creating one is a decision rather than
a side effect of a typo.

The `ETag` follows from all of this. The configuration fingerprint joins the
algorithm version in every tag, because a configuration can now vary and two
worlds behind one URL must not share a validator. A world-backed *image*
carries no tag at all: it also depends on the overlays, which are mutable player
state with no version anywhere in the system, and a tag that ignored them would
go on serving an unexplored map after the player explored it.

**The server and the CLI must agree byte for byte.** Two front ends over one
renderer must not be allowed to drift, and that is one assertion rather than a
second set of goldens: the existing golden image already pins what the renderer
draws, and a second copy of it in the server crate would only pin it twice.

---

### 29.2 Player overlays

Generated terrain and player overlays are stored apart (section 27.6) and meet
in exactly one place: `wgvb_render::render_player`, at render time, in pixels.
Nothing composed there can reach back into generation, and a tile's terrain is
the same whether or not anybody has ever looked at it.

`Overlays` is a plain value — sorted `Vec`s of coordinates and of
`(coordinate, name)` — and `wgvb-render` does not depend on `wgvb-store`. The
two are siblings, and a renderer that could open a database would be a renderer
that could be handed a world rather than a viewport. The CLI does the loading:
one range scan over the smallest `(q, r)` box holding the window's tiles, which
is a superset for a wrapped window and correct for the same reason, since an
overlay outside the window is never drawn.

Sorted rather than hashed, because markers overlap pixels. Section 29 requires a
stable render order, and a `HashSet` would decide which of two touching markers
wins by hash seed.

Two composition rules, and they differ on purpose:

- **Fog hides terrain.** An undiscovered tile is drawn as the fog color rather
  than as what is there, for every layer identically — a fogged tile that leaked
  its heat band would be a map telling the player the climate of ground they
  have never seen.
- **Fog does not hide the player's own marks.** A settlement marker is drawn
  whether or not the tile under it is discovered. It is something the player
  built; hiding it would be the map lying to its owner.

**An empty discovery set means fog is switched off, not that nothing has been
seen.** A world that records no discoveries is one where exploration is not
being tracked, and rendering it as a solid rectangle of fog would be an alarming
way to say so. One discovered tile switches it on.

`RENDER_VERSION` did not move for any of this. Every pixel the terrain renderer
produces is bit-identical to what it produced before overlays existed; bumping
it would have claimed a cache of terrain PNGs was stale when it is not. Note
also what that version does *not* cover: a cached player PNG depends on the
overlays as well as the palette, and overlays are mutable player state with no
version at all. That is a reason not to cache one.

---

## 30. Testing Strategy

Test invariants, not whether a map "looks right".

### 30.1 Determinism

The same seed and coordinate return an identical tile, including every `f64` bit.

### 30.2 Order Independence

Generating a set of coordinates forward, backward, and shuffled produces identical results.

### 30.3 Concurrent Determinism

A `rayon` parallel fill and a sequential fill of the same coordinates agree bit-for-bit. Section 20's no-accumulation rule is what makes this hold; a test that passes here while an accumulation exists is passing by luck.

### 30.4 Negative Coordinates

Test `div_euclid` and `rem_euclid` usage exhaustively around zero for chunk assignment, region assignment, interpolation cell selection, and direction normalization.

### 30.5 Region Boundary Continuity

Elevation, heat, and moisture change smoothly across region anchor boundaries. No step change correlates with a region index change.

### 30.6 Chunk Boundary Continuity

The same, for chunk boundaries. Chunks are addressing only.

### 30.7 Neighbor Coherence

Adjacent tiles have related values. Assert a bound on the distribution of neighbor deltas, not on any individual pair.

### 30.8 Distribution Tests

Over a large sample of coordinates, land fraction, elevation histogram, heat bands, moisture bands, and terrain frequencies fall in expected ranges. These catch a threshold sitting on a knife edge.

### 30.9 Golden Coordinates

A table of representative coordinates and their exact expected tiles, including `f64` bit patterns. Add goldens only for an intentionally stable algorithm version; updating them requires an explicit compatibility decision recorded in the commit message.

Run the golden test on more than one target. It is the only thing that actually proves section 25 is being honored.

### 30.10 Wrapped-Edge Continuity

Compare physical fields on corresponding tiles at all six wrapped edge pairs, using the same continuity expectations as ordinary neighbors. If exact periodicity is not implemented for a field, record the known warp seam explicitly rather than weakening unrelated continuity tests.

### 30.11 Database Compatibility

Verify that opening rejects each of: a non-WGVB application id (including a WGVA file), a schema newer than the binary, an unsupported generator version, malformed or incomplete singleton metadata, a configuration whose fingerprint does not match, and an invalid configuration — each without performing any application write. Verify that supported older schemas migrate in order and retain the same single-world metadata.

Assert on `OpenError` variants, never on message strings.

### 30.12 Type-Level Assertions

The `Send + Sync` assertion from section 22 and a `size_of::<Tile>()` bound belong in the test module as compile-time checks.

---

## 31. Performance Expectations

Tile generation must be cheap enough for interactive scrolling.

The Go design targeted "tens of thousands of tiles per second". Rust with
inlined enum-dispatched fields (section 9.1) should exceed that by one to two
orders of magnitude, so the target is not a useful goal — it is a floor that
signals something is wrong if missed.

Correctness and visual quality come first. Measure before adding any cache or complexity:

```rust
#[bench] fn bench_tile();
#[bench] fn bench_chunk();
#[bench] fn bench_region_radius_32();
```

They live in `crates/wgvb/tests/bench.rs` as `#[ignore]`d `--release` timing
loops rather than as `#[bench]` functions, because `#[bench]` is a nightly
feature and this workspace is pinned to stable:

```sh
cargo test --release -p wgvb --test bench -- --ignored --nocapture
```

Use `cargo bench` with `criterion` if statistical rigor is wanted; a plain
`--release` timing loop is enough to answer the only question that matters early,
which is whether the tile cache in section 27.6 should exist at all.

**Measured, phase 7.** On an M-series laptop, one core:

| measurement | tiles/s | per tile |
|---|---|---|
| `bench_tile` | ~127,000 | 7.9 us |
| `bench_chunk` (32x32 fill) | ~139,000 | 7.2 us |
| `bench_region_radius_32` | ~136,000 | 7.4 us |
| `bench_relief` | ~114,000 | 8.8 us |

Two things to read out of that. The three tile measurements agreeing within ten
percent is the shape a pure function of its own coordinate should have: batching
buys nothing because there was nothing shared to amortize, which is the same
property that makes the batch API safe to parallelize.

The absolute number is the other thing, and it is an honest miss. This section
expected Rust to beat the Go design's "tens of thousands" by one to two orders
of magnitude, and it beats it by well under one. The floor is met and the
correctness invariants are not in question, but a tile is currently about 7.4
microseconds of arithmetic, which for a few dozen `f64` field evaluations is
slow enough to be worth a profile. The likely suspects are the octave ladders in
section 9.3 and the seven elevation evaluations behind `relief` and `Tile`.
**This has not been investigated.** It is recorded here so that the next person
to open the profiler starts from a number rather than from a feeling, and so
that the expectation above is not left standing unqualified.

---

## 32. Initial Implementation Plan

### Phase 1 — Coordinate and hashing foundation

Implement `Coord` with private fields and normalizing constructors, arithmetic
wraparound normalization, axial-to-world conversion, chunk and region
assignment via `div_euclid`, compile-time hash domains, the domain-separated
mixer, and the `Config`/`Generator` skeleton with validation.

> **Exit:** coordinate math, six-edge wrapping, configuration validation, and
> deterministic hashing have complete unit tests. `Coord` cannot be constructed
> non-canonically. The `Send + Sync` assertion compiles.

### Phase 2 — Continuous scalar fields and diagnostic renderer

Implement one owned 2D noise source, the `Field` enum with fbm composition and
domain warping, diagnostic scalar sampling, and an initial `wgvb-map` that
renders scalar layers from an in-memory generator.

> **Exit:** arbitrary canonical coordinates can be sampled and inspected
> visually. Ordinary field sampling has no seams; wrapped-edge continuity is
> best-effort and any remaining warp seam is documented. A golden test runs on a
> second target and passes bit-exactly.

### Phase 3 — Hierarchical region influence

Implement deterministic region parameters, blending across regional anchors, and
regional roughness, climate, basin, and elevation biases.

> **Exit:** distant areas have distinct geographic character with no visible
> implementation-region boundaries.

### Phase 4 — Elevation

Implement continentalness, regional uplift, local relief, sea level, the
elevation scalar, and land/water classification.

> **Exit:** diagnostic elevation maps show coherent oceans, coastlines,
> lowlands, and uplands across multiple windows.

### Phase 5 — Climate

Implement the heat field, elevation cooling, the moisture field, regional
climate bias, and two-axis climate classification.

> **Exit:** climate maps form coherent broad zones rather than tile-level
> speckle.

### Phase 6 — Basins and terrain

Implement broad, regional, and local basin influence; coherent inland water if
and only if it satisfies section 17; terrain classification from physical
fields; and terrain layers in the renderer.

> **Exit:** terrain maps visually correspond to elevation and climate, and
> boundaries look geographically plausible. Basin geography exists; inland water
> is either coherent or deliberately omitted.

**Done at `ALGORITHM_VERSION = 5`.** Inland water was deliberately omitted; see
the decision record in section 17.1. Acceptance renders are in
`docs/renders/v5`.

### Phase 7 — Persistence, player rendering, and tuning

Implement the single-world `rusqlite` database, the migration ladder, all
compatibility gates, canonical configuration persistence and fingerprinting, and
player-facing terrain and overlay PNG composition.

Tune frequencies, weights, thresholds, and warp strengths with the renderer
throughout the preceding phases, then settle the defaults stored for algorithm
version 1.

> **Exit:** a database can create, reopen, and reproduce one world safely.
> Multiple seeds and distant coordinate windows produce varied but coherent maps
> and player PNGs.

---

## 33. Avoid These Designs

### 33.1 Independent random tile classification

Do not derive terrain from `hash(seed, q, r) % TERRAIN_COUNT`. That recreates the incoherent appearance the whole design exists to fix.

### 33.2 Finite global heightmaps

Do not generate a fixed `width x height` array and normalize it. That makes the world bounded.

### 33.3 Mutable PRNG traversal

Do not make tile values depend on the order tiles were generated. In Rust this is largely blocked by taking `&self` everywhere, but a `Cell` or a `Mutex<StdRng>` would defeat it — hence `unsafe_code = "forbid"` and the `Sync` assertion.

### 33.4 Region-owned terrain

Do not assign every tile inside a region one set of hard parameters without blending. That creates seams.

### 33.5 Runtime global normalization

Do not compute min/max elevation or histogram thresholds from the currently explored area. Exploration order would change the world.

### 33.6 Rust-specific traps

- `f64::mul_add` in the generation path (section 25.1).
- Transcendental functions from `std` in the generation path (section 25.2).
- `DefaultHasher` for anything persisted (section 25.4).
- `#[serde(default)]` on a generation-affecting config field (section 21.1).
- A noise crate with runtime SIMD dispatch (section 9.2).
- Plain `*` and `+` in a hash mixer (section 8.2).
- Reducing over a `HashMap` or `HashSet` (section 25.3).
- `hexx` layout math reaching the generation path (section 7.1).

---

## 34. Future Extensions

### 34.1 Rivers

Derive deterministic watershed structure from a coarser hydrology field, trace river paths locally from stable source features, and ensure any path can be reconstructed from coordinates alone. Deferred because globally coherent drainage is substantially harder than scalar field generation.

### 34.2 Biomes

Terrain classification can evolve into richer biome classification.

### 34.3 Resources

Resources use the same hierarchical deterministic path approach: `seed + resource domain + region + coordinate`.

### 34.4 Named geographic features

Macro regions can provide deterministic identities for mountain systems, deserts, forests, and seas. Names are a separate layer from physical generation.

### 34.5 Alternative world topology

The current design uses the finite hexagonal wraparound topology in section 7.1. A future generator version could introduce a cylinder, torus, sphere-like topology, or a differently sized wrapped world. Coordinate normalization must remain a distinct layer from physical fields and classification so such a change is possible. Topology is an algorithm compatibility decision and cannot change for an existing world.

---

## 35. Coding-Agent Guidance

1. Prefer small deterministic functions over stateful generator steps.
2. Keep raw scalar fields separate from classification.
3. Expose diagnostic sampling early.
4. Build the renderer before extensive aesthetic tuning.
5. Test negative coordinates from the beginning.
6. Do not optimize before profiling.
7. Do not add a dependency to `crates/wgvb` without a stated reason in the PR.
8. Treat changes that alter existing generated worlds as versioned algorithm changes.
9. Keep all geographic boundaries emergent; implementation regions and chunks must remain invisible.
10. Read section 25 before writing any floating-point code.
11. Preserve the central invariant:

```text
Tile = F(seed, q, r, algorithm_version, configuration)
```

with no dependency on generation order or previously generated tiles.

---

## 36. Definition of Success

The first major WGVB milestone is complete when all of the following hold:

- A caller can request any axial coordinate `(q, r)`.
- The fixed wrapped boundary is never encountered as a terminal gameplay edge.
- The returned tile contains normalized elevation, heat, moisture, and relief values plus elevation, climate, and terrain classifications.
- The same seed and coordinate always return the same tile, bit-for-bit, on every supported target.
- Adjacent tiles form visually coherent geographic features.
- Large-scale terrain differs across distant portions of the map.
- Region and chunk boundaries cannot be identified by looking at generated terrain.
- Negative coordinates work correctly.
- All six world edges wrap correctly; any best-effort geographic discontinuity is documented as a world-warp seam.
- The world can be rendered in arbitrary windows for inspection.
- Generating a distant tile does not require generating the intervening world.
- A non-canonical `Coord` cannot be constructed.

At that point WGVB will retain the operational simplicity of the original Marajanda coordinate-path generator while producing terrain with the visual coherence of a modern noise-based map.

---

## Appendix A — Direction Vectors

The six canonical directions are numbered `0` through `5`, in the order Red Blob Games gives them. Increasing the index by one steps to the next neighbor **counter-clockwise**; decreasing it steps clockwise. This ordering is independent of whether a renderer draws flat-top or pointy-top hexes.

**"Clockwise" is a word about a picture, and this document uses it for exactly one thing:** walking the ring of neighbors as a viewer sees them, starting at that viewer's north and proceeding N, NE, SE, S, SW, NW. See *Rotation senses* below, which is the section to read before writing the word in a comment.

The six vectors are pinned by the algorithm version. They are world data, not a
rendering detail, because the per-player rotation below is defined as arithmetic
over these indices:

| Direction | Cube `(q, r, s)` | Axial `(q, r)` |
|---:|---|---|
| 0 | `(+1,  0, -1)` | `(+1,  0)` |
| 1 | `(+1, -1,  0)` | `(+1, -1)` |
| 2 | `( 0, -1, +1)` | `( 0, -1)` |
| 3 | `(-1,  0, +1)` | `(-1,  0)` |
| 4 | `(-1, +1,  0)` | `(-1, +1)` |
| 5 | `( 0, +1, -1)` | `( 0, +1)` |

The cube form is `(q, r, -q - r)`. Changing the table, or renumbering it, changes
every world and is an algorithm compatibility change.

**Directions carry no compass names in the `wgvb` crate.** North is a property of
a viewing player, not of the world; see *Coordinate frames* below.

Callers may supply any integer direction. Normalize before indexing:

```rust
#[inline]
fn normalize_direction(dir: i32) -> usize {
    dir.rem_euclid(6) as usize
}
```

The Go design needed a five-line helper here because Go's `%` returns a remainder with the sign of the dividend, so `-7 % 6` is `-1`. Rust's `rem_euclid` is always non-negative for a positive divisor, so the helper is one expression and the adjustment branch disappears.

Values differing by a multiple of six identify the same direction:

| Input | Normalized | Movement from direction 0 |
|---:|---:|---|
| `7` | `1` | One step in index order |
| `6` | `0` | Full turn |
| `-1` | `5` | One step against index order |
| `-2` | `4` | Two steps against index order |
| `-6` | `0` | Full turn |

Direction iteration in any accumulating context must use fixed order `0..6`. See section 25.3.

### Rotation

One step in index order — direction `d` to `d + 1` — is an exact permutation with
sign changes on the cube form:

```rust
const fn rotate_once(v: (i64, i64, i64)) -> (i64, i64, i64) {
    let (x, y, z) = v;
    (-z, -x, -y)
}
```

`rotate_once(direction[d]) == direction[(d + 1) % 6]` for all six, and six
applications are the identity. The inverse step is `(x, y, z) -> (-y, -z, -x)`.
`Coord::rotate(steps)` applies it `steps` times, for any signed `steps`.

**Both are named for the index, not for a rotation sense**, and that is
deliberate. `rotate_once` was `rotate_cw` and `Coord::rotate` was
`Coord::rotate_cw`, which misled twice over: the step is clockwise only under a
plot of canonical world space with `+y` upward, which nothing here draws, and
`cw` reads as either "clockwise" or "compass walk" now that both exist and run
opposite ways. **Do not reintroduce the abbreviation in either sense.** Write
"index order" or write the compass names out.

This is integer-exact, so it is available everywhere in the generation path
without violating section 25.2 — there is no rotation matrix and no angle.

**The same function generates the mirror centers.** `MIRROR_CENTERS` in section
7.1 is the six rotations of `(2N+1, -N, -N-1)` under `rotate_once`, so write the
rotation once and derive the table from it rather than transcribing six triples.
Two of the six centers have components at `±65535`, outside `Component` range,
which is what the `i64` intermediate rule in section 4 protects.

Because the canonical domain is six-fold symmetric about the origin, the
rotation maps it onto itself: it commutes with normalization.

### Rotation senses

Two different rotations are in play and they run opposite ways. Conflating them
has already cost one round of confusion, so both are written out here.

**Index order is counter-clockwise.** The table above is Red Blob Games' order:
`direction[0]` is `(+1, 0, -1)` and `direction[1]` is `(+1, -1, 0)`, and as any
viewer sees the world, that second vector is one sixth of a turn
*counter-clockwise* from the first. `Coord::rotate(1)` and the `d + 1` in
`neighbor(d + 1)` both move that way.

**The compass ring is clockwise.** A viewer's six neighbors, named the way a
person names them, are N, NE, SE, S, SW, NW — a clockwise walk that starts at
whatever direction is *that viewer's* north. Because index order runs the other
way, the compass walk **decreases** the index:

| Compass | Absolute direction | Player-relative direction |
|---|---|---|
| N  | `k`     | `0` |
| NE | `k - 1` | `5` |
| SE | `k - 2` | `4` |
| S  | `k - 3` | `3` |
| SW | `k - 4` | `2` |
| NW | `k - 5` | `1` |

all `mod 6`, for a viewer at rotation `k`. The player-relative column is the
same for every viewer, which is the point: **the clockwise compass walk is
always `0, 5, 4, 3, 2, 1` in the viewer's own numbering**, whatever their
rotation is, and `3` is always behind them.

**The admin frame is rotation 2.** The diagnostic renderer applies no rotation,
and its flat-top layout puts absolute direction `2` at the top of the image, so
what it draws is a viewer at `k = 2`. The compass walk there is absolute
`2, 1, 0, 5, 4, 3`:

| Compass | N | NE | SE | S | SW | NW |
|---|---:|---:|---:|---:|---:|---:|
| Absolute direction | 2 | 1 | 0 | 5 | 4 | 3 |
| Axial step | `(0, -1)` | `(+1, -1)` | `(+1, 0)` | `(0, +1)` | `(-1, +1)` | `(-1, 0)` |

Rules that follow:

- **The generator says neither word.** Nothing under `crates/wgvb` has a north,
  so nothing under it needs "clockwise": the core crate deals in direction
  *indices* and the arithmetic `(d + 1) mod 6`. A comment there that says
  clockwise is describing a picture the crate cannot see.
- **Presentation and player-facing text say only "clockwise", never an index
  direction.** A heading printed to a player, a compass rose, a scroll control,
  a "turn right" — all of them mean the compass walk above, which is index
  minus one.
- Both senses are exact integer arithmetic on indices. Neither is an angle, and
  section 25.2 is untroubled by either.
- **Never abbreviate either one to `cw`.** With both senses named, the two
  letters read as "clockwise" or as "compass walk", which are opposite
  directions through the index — the worst possible ambiguity in the shortest
  possible identifier. Nothing in the workspace is named `cw` or `ccw`, and
  nothing should be. Say "index order" or name the compass points.

Also resist describing the presented frame by its handedness. It is true that
plotting a `+y`-up plane into a `+y`-down raster reverses the apparent sense of
a turn, and it is true that this is what puts index order and the compass walk
on opposite paths — but "the image frame is left-handed" invites the reader to
conclude that the compass walk is backwards, when the compass walk is the
ordinary right-handed one and reads exactly as a person expects. Describe what
is seen: index up is counter-clockwise, the compass runs clockwise, and the two
therefore disagree by a sign.

### Coordinate frames

Two distinct frames exist, and confusing them is a real hazard.

**Canonical, or absolute.** The wrapped hexagonal domain of section 7.1. This is
the only frame the generator ever sees, the only frame used as a persistence key,
and what `Coord` represents. Unless a passage says otherwise, coordinates in this
document are canonical.

**Player-relative.** Every player is assigned an origin hex and a rotation when
they are created, and sees the world through that frame on a **flat-top** layout.
One player's `(0, 0)` is not another's, and their norths may differ: rotation is a
direction offset, so a player at rotation `k` perceives absolute direction `k` as
north. Two players can describe the same tile with different coordinates and the
same heading with different direction numbers.

The presented layout is pinned, because "north" is meaningless without it:
**flat-top hexes, image `y` increasing downward, the frame's north at the top of
the image.** Flat-top is what makes north a neighbor at all — a flat-top hex has
neighbors directly above and below it, a pointy-top one does not — so the six
compass names in *Rotation senses* exist only in this layout.

The transform is exact integer arithmetic:

```text
absolute = normalize( rotate^k ( relative ) + player_origin )
absolute_direction = (player_direction + k) mod 6
```

Rules:

- A player frame must never reach the `Generator`. Fields are sampled on canonical
  coordinates only, or `Tile = F(seed, coordinate, version, configuration)` would
  acquire a per-player term.
- Player origin and rotation are authoritative player state, never generated data.
- Compass names, and the conversion in either direction, belong to the rendering
  and presentation layers.

In discussion the shorthand `(q, r)` often means a player-relative coordinate and
`(q, r, s)` an absolute one. This document does not rely on that convention:
canonical is the default, and player-relative coordinates are always labeled.

---

## Appendix B — Dependency Map

Every version is pinned once in the workspace root `Cargo.toml` under `[workspace.dependencies]`. Member crates opt in with `dep.workspace = true`, so no two crates can drift.

| WGVA (Go) | WGVB (Rust) | Notes |
|---|---|---|
| `maloquacious/hexg` | `hexx` | `wgvb-render` only. Layouts are `f32` via `glam`; see section 7.1. |
| unspecified noise | *none — owned* | Section 9.2. |
| `zombiezen.com/go/sqlite` | `rusqlite` (`bundled`) | Same idiom: direct, non-ORM. |
| `sqlitemigration` | *none — owned* | ~40-line `user_version` ladder; section 27.4. |
| `image/png` | `png` | Golden-compare decoded RGBA; section 29. |
| `flag` | `clap` (derive) | — |
| `net/http` | `tiny_http` | `wgvb-serve` only; section 29.1. |
| `math/rand/v2` | `rand` | Non-generation use only; section 8. |
| `crypto/sha256` | `sha2` | Fingerprint; section 21.2. |
| `encoding/json` | `ciborium` | Canonical config bytes; section 21.2. |
| — | `serde` | Config and `Field` serialization. |
| — | `thiserror` | Typed error enums; section 19.1. |
| — | `rayon` | Batch fills only; section 20. |

The core `wgvb` crate depends on exactly two of these: `serde` and `thiserror`.
Keep it that way.

`wgvb-serve` additionally appears as a `wgvb-map` **dev**-dependency, and that
arrow points backwards on purpose. Section 29.1 requires the two front ends to
render identical bytes, and the test that asserts it has to run the real
`wgvb-map` binary, which only that package's own tests can locate. No cycle
exists — `wgvb-serve` does not depend on `wgvb-map` — and it does not appear in
a consumer's graph.

`ciborium` additionally appears as a `wgvb` **dev**-dependency. Proving that
section 21.1's `deny_unknown_fields` and no-`serde(default)` rules actually fire
needs a real serialization format, and this is the format section 21.2 chose. It
is not a dependency of the library and does not appear in a consumer's graph.

---

## Appendix C — Storage Decision Record

**Decision: SQLite via `rusqlite`, with `WITHOUT ROWID` coordinate-keyed tables.
A dedicated B-tree key-value store was evaluated and rejected for authoritative
data.**

### The workload

Reading section 27, the store handles three payloads: singleton world metadata
(one record), sparse mutable overlays keyed by canonical `(q, r)`, and
reproducible caches keyed by coordinate or chunk. Access is point lookup and
range scan by region. There is not a single join in this document. That is a
key-value workload wearing a SQL costume, and the question deserved a real
answer rather than an inherited default.

### What was evaluated

| Candidate | Verdict |
|---|---|
| **`redb`** | The right pick *if* going KV. Pure Rust, copy-on-write B-tree, ACID, MVCC with one writer and unblocked concurrent readers, typed tables, zero-copy reads, single file. |
| **`heed`** (LMDB) | Solid and zero-copy, but a C dependency and `map_size` must be pre-sized and grown. |
| **`sled`** | Rejected. The on-disk format has churned and 1.0 has been pending for years — disqualifying for a file that must open in five years. |
| **`fjall`** | Rejected. LSM, tuned for write-heavy workloads; wrong shape for read-and-scan. |

### Why SQLite wins here

1. **File format longevity.** SQLite's on-disk format has been stable since 2004
   with a published commitment through 2050. `redb` has its own internal format
   version, and a `redb` major release can require a format migration *on top of*
   our schema migrations — two formats to version instead of one, for data
   measured in megabytes.
2. **Inspectability.** Any world file opens in the `sqlite3` CLI. Debugging a
   corrupt world from a player's bug report is a `SELECT`, not a custom dump
   tool that has to be written and kept working.
3. **`WITHOUT ROWID` already gives us the B-tree.** The coordinate locality that
   motivates a KV store is available inside SQLite (section 27.2). The remaining
   KV advantages — no SQL parse/bind/step per row, zero-copy mmap reads — matter
   for bulk cache writes and essentially nothing else here.
4. **Query patterns will grow.** Overlays are coordinate-keyed today. "All
   settlements owned by player X", "everything changed since turn N", "labels
   within this chunk range" are each a `WHERE` clause in SQLite and a
   hand-maintained, transactionally-consistent secondary index in `redb`.
5. **The authoritative data is small.** Metadata plus settlements, labels, fog,
   and discoveries. SQLite's throughput is irrelevant at that size; its tooling
   and durability record are not.

### What SQLite costs us

Honestly: `redb`'s MVCC model is a better match for section 22's
immutable-generator design than `Connection: !Sync` and manual connection
handling. That cost is real but small, because the hot path — generation —
touches no database at all (section 27.7).

### If this is revisited

The place where a KV store genuinely wins is the tile/chunk cache, and section
27.6 already declares that disposable and defers building it. If profiling ever
justifies one, the clean answer is a **separate `redb` file beside the world**:
disposable data in a disposable-format store, authoritative data in SQLite. Do
not migrate the authoritative side.

Two other conditions would reopen the decision:

- **A WebAssembly target.** Bundled SQLite in the browser is genuinely painful;
  `redb` is pure Rust. If a browser renderer becomes a goal, re-evaluate.
- **A hard requirement for a pure-Rust dependency tree** (no C toolchain in the
  build).

If `redb` is ever adopted, the required changes are bounded and known:

- The four pragma-based gates become a reserved `meta` table: `magic` →
  `0x5747_5642`, `format_version` → `u32` replacing `user_version`, plus
  `generator_version`, `seed`, `config`, `config_fingerprint`. The **six-step
  gate order in section 27.5 does not change.**
- Write a hand-rolled `redb::Key`/`redb::Value` impl for `Coord` rather than
  relying on tuple impls, so on-disk key bytes are pinned by our code and cannot
  shift under a `redb` upgrade. Encode each `i16` big-endian with the sign bit
  flipped (`(q as u16) ^ 0x8000`) so lexicographic byte order matches numeric
  order and range scans work.
- Pin the `redb` version exactly and treat a major upgrade as a format migration.
- Sections 30.11 and 27.3 survive unchanged in substance.
