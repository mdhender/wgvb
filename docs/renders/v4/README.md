# Acceptance renders — algorithm version 4

Contact sheets for phase 5, climate (issue #6). The phase's exit condition is a
judgement — *climate maps form coherent broad zones rather than tile-level
speckle* — and no assertion settles it, so these are the evidence it was met.
The measurements in `crates/wgvb/tests/climate.rs` are the other half: they say
the bands are occupied, the neighbors agree, and high ground is colder. They
cannot say the world looks like a world.

Nothing in the build reads these files.

All sheets use seed `0x0123_4567_89ab_cdef`.

## The wide sheets

`climate.png`, `temperature.png`, and `moisture.png` each show the same four
widely separated windows, arranged two by two, reading left to right and top to
bottom:

| Panel | First tile |
|---|---|
| top left | `(-600, -450)` |
| top right | `(11000, -4500)` |
| bottom left | `(-7000, 2200)` |
| bottom right | `(2500, 9100)` |

Every window is 1,201 by 901 tiles at a hex radius of one pixel — about 7,200
miles across, a little over half the wavelength of the broad heat field. That is
the scale climate is built at, and a window small enough to show a coastline is
too small to show a climate zone at all: the `climate` row of the golden image
table in `wgvb-render` is one flat color for exactly that reason.

```sh
wgvb-map --seed 81985529216486895 --q -600 --r -450 \
    --cols 1201 --rows 901 --hex-radius 1 --layer climate --out climate-1.png
```

What to look for:

- **Zones, not speckle.** Bands cover hundreds of hexes and their boundaries
  wander. The fine texture inside a moisture band is the 100-hex local variation
  term of section 16, and it is 25 hexes across at its smallest, not one.
- **Two axes that disagree.** The temperature and moisture sheets are not the
  same picture. If they were, the two-axis model would be decoration.
- **No lattice.** The region heat and moisture biases are blended from anchors
  every 128 and 512 hexes. A blend that had collapsed would draw a grid of
  parallelograms at those spacings, which at this zoom would be unmistakable.
- **Cold high ground.** Every mottled, filigreed patch on the temperature sheet
  is a mountain range, and there is no mountain term in the heat field — that is
  elevation cooling, and it is the only thing in the composite with edges that
  sharp. In the two warm panels on the left it reads as pale threads against
  green, because the shared scalar ramp puts its shore color at zero and a
  cooled summit in a warm zone lands near there. In the two cold panels on the
  right the same ranges read as darker blue against blue. Rendering `--layer
  elevation` over any of these four windows puts the same shapes in the same
  places, which is how the claim was checked rather than assumed.

The scalar layers borrow the elevation ramp, which is deliberate — one ramp for
every scalar layer, so two of them can be compared by eye — but it does mean
those images read as coastlines until you remember that blue is *cold* on one
and *dry* on the other. Climate has its own table: wetter is greener across the
image, warmer is redder down it.

## The cooling sheet

`cooling.png` is one window at a hex radius of four pixels, close enough to see
individual tiles, in three layers: elevation, then temperature, then climate.

```sh
wgvb-map --seed 81985529216486895 --q 2540 --r 9090 \
    --cols 201 --rows 151 --hex-radius 4 --layer elevation --out close-elevation.png
```

This is the attribution the wide sheets cannot make. The land rises from a coast
on the left to upland on the right; the temperature panel darkens the same way,
and the climate panel reads cold on the coast and polar on the high ground. The
moisture band changes across this window too, and it changes for its own
reasons — that is the two axes being independent, not cooling reaching the wrong
one. Nothing in the heat field knows where this coastline is: the cooling term
is the only thing in either composite that reads elevation at all.

`tests/climate.rs` makes the same claim twice over as a measurement, once as
*no sloped pair of land neighbors is warmer uphill* and once by generating the
same seed with cooling disabled and comparing the two populations.

## What did not move

Every elevation and relief value is bit-for-bit what version 3 produced, and the
golden tables in `crates/wgvb/tests/golden.rs` did not have to be re-recorded —
climate reads elevation and does not feed it. `ALGORITHM_VERSION` moved to 4
anyway, because `Config` gained the climate settings and a world file written
under version 3 does not carry them. The phase 4 sheets in `../v3` are therefore
still pictures of this world.
