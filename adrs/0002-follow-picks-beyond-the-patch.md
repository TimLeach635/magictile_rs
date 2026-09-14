# 2. Follow picked vertices beyond the generated patch

- **Status:** Accepted
- **Date:** 2026-09-14

## Context

The two vertices picked for a measurement were held as indices into the generated patch.
Recentering carried them across by its symmetry, and if either landed outside the patch, both were
dropped and the measurement with them.

Measurements span the screen, so after a pan the first pick typically sits near the edge of the
patch, and panning a little further made the measurement vanish. Since
[ADR 0001](0001-confirmed-shortest-distances.md), measuring no longer needs the picks to be in the
patch at all, only each pick's label and position.

## Decision

A pick (`cayley::Pick`) is a label, a position in tiling coordinates, and the position of the next
vertex around its q-gon, which fixes its orientation.

- **Recentering** onto a vertex `v`, by the symmetry taking the home vertex onto `v`, moves a pick's
  position and orientation point by the inverse of that symmetry, and multiplies its label on the
  left by `label(v)⁻¹`. The view's accumulated renaming is multiplied on the right by `label(v)` at
  the same time, so the permutation the pick shows is unchanged.
- **Settling back.** Whenever a carried pick lands on a vertex of the patch carrying the same label,
  it takes that vertex's exact data, discarding any rounding gathered while it was off the patch.
- **Measuring** builds the symmetry taking the home vertex onto the first pick from its position and
  orientation point, then searches as in ADR 0001.
- **Reach.** A pick is dropped only once it is more than 12 from the origin, and both picks are
  dropped together. That far out a point's disc coordinate is within about 1.2 × 10⁻⁵ of the rim,
  and beyond it the rounding in the Möbius transformations starts to approach the positions
  themselves.
- **Markers.** A pick panned out of sight is left unmarked, rather than drawn in the middle of the
  screen.

## Alternatives considered

- **Hold the symmetry taking the home vertex onto each pick**, composing it with every
  recentering. Equivalent, but it composes many transformations over a long pan. Carrying points
  directly is simpler and settles back onto exact vertex data.
- **No reach limit.** Positions nearer the rim than double precision can resolve would give
  meaningless distances.
- **Hold far-off picks exactly**, as an element of the surface's deck group together with a vertex
  of the home unit. That would remove the limit, but it needs exact arithmetic in the surface group,
  which the viewer does not have, and it would only matter for picks more than 12 away, far off
  screen.

## Consequences

- A measurement survives panning well beyond the patch. In a scripted pan the first pick was carried
  from 3.31 to 10.52 from the origin: past where the patch is complete (about 5.5) and past the
  whole patch (about 7.5). The distance read the same confirmed 2.4376 throughout. The next drag
  carried it past 12, and the measurement was dropped.
- `picks_are_followed_beyond_the_patch` pans two picks outwards until they are dropped. At every step
  it checks they show the same permutations and the measurement is the same confirmed distance. It
  follows them out to 11.17, with both off the patch for the last two steps.
- Self-test dumps record each pick's distance from the origin.
