# Acceptance renders — algorithm version 5

Contact sheets for phase 6, basins and terrain (issue #7). The phase's exit
condition is a judgement — *terrain maps visually correspond to elevation and
climate, and boundaries look geographically plausible* — and no assertion
settles it, so these are the evidence it was met. The measurements in
`crates/wgvb/tests/terrain.rs` and `crates/wgvb/tests/basin.rs` are the other
half: they say every terrain is reachable, none dominates, neighbors agree,
coast only ever appears at the water's edge, and no tile anywhere is inland
water. They cannot say the world looks like a world.

Nothing in the build reads these files.

All sheets use seed `0x0123_4567_89ab_cdef`.

## The wide sheets

`terrain.png`, `basin.png`, and `volcanic.png` each show the same four widely
separated windows, arranged two by two, reading left to right and top to
bottom — the same four windows the version 4 sheets use, so the two phases can
be compared panel for panel:

| Panel | First tile |
|---|---|
| top left | `(-600, -450)` |
| top right | `(11000, -4500)` |
| bottom left | `(-7000, 2200)` |
| bottom right | `(2500, 9100)` |

Every window is 1,201 by 901 tiles at a hex radius of one pixel — about 7,200
miles across.

```sh
wgvb-map --seed 81985529216486895 --q 11000 --r -4500 \
    --cols 1201 --rows 901 --hex-radius 1 --layer terrain --out terrain-2.png
```

What to look for on `terrain.png`:

- **The same shapes as the other two layers.** `../v4/climate.png` is the same
  four windows in the same order, so the two sheets can be read side by side:
  every ice cap here sits inside a polar zone there, and every belt of boreal
  forest inside a cold one. Rendering `--layer elevation` over any of these
  four windows puts the coastlines in the same places. That is how the
  correspondence was checked rather than assumed, and `close.png` below is the
  three layers of one window at a zoom where individual tiles are visible.
- **Boundaries that follow the ground.** Terrain changes along lines — a
  shoreline, the foot of a range, the edge of a climate zone — rather than at
  points. A classifier that had degenerated into `hash(seed, q, r) %
  TERRAIN_COUNT` (section 33.1) would produce a perfectly balanced
  distribution and a picture of static, which is why the neighbor-agreement
  measurement matters more than the distribution one.
- **A shoreline one tile wide.** Coastal water (pale blue) rings every
  landmass and coast (cream) lines the land side of it. Open ocean never
  touches open land; a test asserts that, and it is visible here as the
  unbroken double fringe.
- **Volcanoes are dots.** The dark red patches are volcanic highland, a
  province rather than a cone; the bright red single tiles inside them are
  volcanoes. There are a handful per continent, and no two are adjacent —
  which follows from a volcano being a strict local maximum of the elevation
  field.
- **No lattice.** Region anchors sit every 128 and 512 hexes. A blend that had
  collapsed would draw a grid of parallelograms at those spacings, which at
  this zoom would be unmistakable.

`basin.png` and `volcanic.png` are the two new scalar layers, at half the
scale. Both borrow the shared elevation ramp, which is deliberate — one ramp
for every scalar layer, so two of them can be compared by eye — and it does
mean the images read as coastlines until you remember that on `basin` the
green is *enclosed low ground* and the blue is *a rise*.

The thing to check on `basin.png` is that it is **not** a picture of the
coastlines. Basin influence says how enclosed a place is, not how high it is,
and the two are built from different domains at different wavelengths; a basin
layer that traced the shore would mean elevation had leaked into it. A
measurement says the same thing — the two correlate under 0.35 — but the image
is the thing that makes it obvious.

## The close sheet

`close.png` is one window at a hex radius of four pixels, close enough to see
individual tiles, in three layers: elevation, then climate, then terrain.

```sh
wgvb-map --seed 81985529216486895 --q 2540 --r 9090 \
    --cols 201 --rows 151 --hex-radius 4 --layer terrain --out close-terrain.png
```

This is the correspondence the wide sheets are too coarse to show. The land
rises from a coast on the left to upland on the right; the climate panel reads
cold on the coast and polar on the high ground; and the terrain panel puts
boreal forest on the cold coastal lowland, tundra and hills across the middle,
and glacial ice on the polar high ground in the bottom right. The rust threads
through the uplands are badlands on the dry exposed relief, and the red dots
in the lower left are volcanoes.

Read the three panels in order and the rule order of section 17 is visible:
ice beats the tundra the climate cover would have chosen, the elevated rules
beat the cover where the ground is broken, and the cover fills in everything
that is left.

## Inland water is not here

There are no lakes on any of these sheets, and that is a decision rather than
a gap. `DESIGN.md` section 17.1 is the record: a coherent lake needs one
surface elevation shared by every tile of one basin, and knowing which tiles
those are is connectivity, which sections 2.3 and 15 forbid. What ships
instead is the basin geography, and `basin.png` beside `terrain.png` shows
what it buys: a wet basin reads as marsh, swamp, or bog, and a dry one as the
pale open interior of a continent rather than as a hole in it.

## What did not move

Every elevation, relief, heat, and moisture value is bit-for-bit what version 4
produced, and the golden tables in `crates/wgvb/tests/golden.rs` did not have
to be re-recorded — terrain reads those fields and does not feed them, and
basin influence deliberately stays out of the elevation composite.
`ALGORITHM_VERSION` moved to 5 anyway, because `Config` gained the basin and
volcanic settings and the terrain thresholds, and a world file written under
version 4 does not carry them. The phase 4 sheets in `../v3` and the phase 5
sheets in `../v4` are therefore still pictures of this world.
