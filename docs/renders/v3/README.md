# Acceptance renders — algorithm version 3

Contact sheets produced for the two changes that bumped `ALGORITHM_VERSION` from
2 to 3: the per-field fbm octave counts bounded at the tile grid's Nyquist
wavelength (issue #11) and the seed-derived sampling offset that moves the world
origin off the noise lattice (issue #10).

They are kept because issue #11 asked for the change to be accepted on the
images rather than on a green test suite. A rendered window is not evidence of
either fix on its own — the measurements in `crates/wgvb/tests/world_origin.rs`
and `crates/wgvb/tests/elevation.rs` are — but they are the evidence that the
world did not get worse to look at, and that is a judgement no assertion makes.

All four sheets use seed `0x0123_4567_89ab_cdef` and the same four widely
separated windows at two zooms: `wide` is 300 by 220 tiles at two pixels per hex
radius, `close` is 80 by 60 at eight.

| Sheet | Left column of each pair | Right column |
|---|---|---|
| `elevation.png` | version 2 | version 3 |
| `relief.png` | version 2 | version 3 |
| `ladder-elevation.png` | version 3 with a five-octave ladder | version 3 as shipped |
| `ladder-relief.png` | version 3 with a five-octave ladder | version 3 as shipped |

The first two pairs are **not the same ground**. The sampling offset moves every
field, so a window at version 3 shows different terrain from the same window at
version 2; what those sheets compare is the character of the world, not tile
against tile.

The `ladder-*` sheets hold the world fixed and change only the octave counts, so
those pairs *are* the same ground and are the controlled comparison for #11. The
difference is visible in relief at the close zoom: the same ridge lines and the
same rough ground, in coherent patches rather than under a layer of per-tile
speckle. Elevation barely moves, which is the point — the aliased octaves
carried a sixteenth and a thirty-second of their ladder's amplitude, and relief
is a first difference between neighbors, which is what amplified them.

Regenerating these is a matter of running `wgvb-map` over the windows twice;
nothing in the build reads them.
