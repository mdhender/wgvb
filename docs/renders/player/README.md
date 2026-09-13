# Acceptance renders — player composition

Evidence for phase 7 (issue #8). The phase's exit condition is partly a
judgement — *multiple seeds and distant coordinate windows produce varied but
coherent maps and player PNGs* — and the composition rules in `DESIGN.md`
section 29.2 are a design decision that a passing test suite cannot show you.
These are the pictures.

Nothing in the build reads these files.

All four use seed `0x0123_4567_89ab_cdef`, the 150 x 110 window whose first
tile is `(11000, -4500)` — the top-right panel of the version 5 contact sheets —
and the `terrain` layer, so the four differ only in what the player knows.

| File | What it shows |
|---|---|
| `terrain.png` | The world as the generator makes it. No database involved. |
| `saved.png` | The same window rendered from `world.wgvb`. Identical, which is the point. |
| `explored.png` | Fog of war: a radius-34 disc around `(11075, -4482)` discovered, everything else unknown. |
| `settled.png` | Two settlements, one inside the explored disc and one out in the fog. |

## What to look at

**`terrain.png` and `saved.png` are the same image.** That is the round trip:
the database supplies the seed and the complete effective configuration, the
generator built from them produces the same tiles, and nothing about having
saved a world changes what the world is. A test asserts it on decoded RGBA; this
is the same claim in a form a person can check.

**`explored.png` hides terrain and nothing else.** The fog is flat and neutral
rather than a darkened terrain, because a darkened ocean still tells you it is
an ocean. The boundary is the hex disc that was discovered, not a circle in
pixels: fog is a property of tiles.

**`settled.png` draws the marker outside the disc too.** Fog hides the world;
it does not hide what the player built. Ashford sits on the coast inside the
explored disc; Longwatch, down at the lower left, sits on ground the player has
never surveyed and is drawn anyway, because a player whose own town vanished
from their map would be reading a lie. See section 29.2 for why the two rules
differ.

The markers are small on purpose. They scale with the hex radius — 40 percent
of it, floored at one pixel — so a mark stays a mark on a tile rather than
swallowing the tile at one zoom and vanishing at another.

Note also what `explored.png` implies about the empty case: `terrain.png` and
`saved.png` come from a database with no discoveries recorded at all, and they
are not fogged. An empty discovery set means exploration is not being tracked,
not that nothing has been seen.

## Reproducing them

```sh
cargo build --release -p wgvb-map
W="--q 11000 --r -4500 --cols 150 --rows 110 --hex-radius 8 --layer terrain"

target/release/wgvb-map --seed 81985529216486895 $W --out terrain.png
target/release/wgvb-map --db world.wgvb --seed 81985529216486895 $W --out saved.png
target/release/wgvb-map --db world.wgvb \
    --discover 11075,-4482 --discover-radius 34 $W --out explored.png
target/release/wgvb-map --db world.wgvb \
    --settle 11075,-4482=Ashford --settle 11030,-4425=Longwatch $W --out settled.png
```

The `world.wgvb` those commands create is not kept here: it is 72 KB of
regenerable file whose whole content is the seed and the defaults, and the
fourth command's output is the proof it round-tripped.

The third and fourth commands write player state before they render, so running
them out of order gives a different picture — which is the difference between
generated terrain and authoritative player state in one sentence.
