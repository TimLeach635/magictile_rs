# 1. Confirm shortest distances, measuring where the generated patch is widest

- **Status:** Accepted
- **Date:** 2026-09-14

## Context

The tiling viewer measures between two picked vertices. Every permutation repeats once per
repeating unit (for t{4,5}, the 20-gon), so the shortest way between two vertices may run to a
*copy* of the target rather than the vertex picked. Finding that copy means searching the tiling,
and the viewer only generates a finite patch of it: a disc of hyperbolic radius about 7.5 around
the origin, complete out to about 5.5.

The original search looked for copies around the picked vertex itself, and near the edge of the
patch that went wrong. Panning recenters the view, which moves where the picked vertices sit within
the patch, so a nearer copy could fall off the edge and the answer fall back to a worse one. In one
pan a pair read 2.6211 apart (7 hops, wrapping around) and then, a little further along, 2.9387
(8 hops, not wrapping): the distance, the hop count and the wrapping all flickered.

This is not a rare corner. A measurement spans the screen, and recentering only keeps the *middle*
of the view near the middle of the patch. After a pan, the first vertex picked is typically out
near the rim of the disc and near the edge of the patch.

A shortest distance should be *known* to be the shortest, not just stable.

## Decision

### Measure around the home vertex

The labelling has a symmetry taking the home vertex onto any vertex `v`. It preserves the tiling and
renames every label `x` as `label(v) · x` (the viewer's recentering already relies on this). Undo
the one for `from`, and the picture moves to where the patch is widest:

- `from` lands on the home vertex, 0.58 from the origin;
- the copies of `to` land on the vertices labelled `label(from)⁻¹ · label(to)`, all around it.

Search there, then carry the results back. The nearest copy and the route come back as *positions*
rather than vertex indices, so they can be drawn even where they lie beyond the generated patch.
Implemented as `Cayley::measure`.

### Confirm the distance

Measuring around the home vertex makes confirmation all but certain, but it is still checked
(`Cayley::nearest_copy`):

1. Find the nearest copy among the patch's vertices. Call its distance `R`.
2. The answer is confirmed if no *incomplete* vertex lies within `R + D` of where the search
   started. An incomplete vertex is a labelled vertex missing one of its three neighbors, which is
   where the patch stops. `D` is the furthest apart two vertices of one face are (1.1689 for
   t{4,5}).
3. Otherwise `R` is only an upper bound, and the viewer says "at most".

**Why this is sound.** Suppose a copy nearer than `R` were missing from the patch. The geodesic
out to it passes through a chain of faces, each sharing a vertex with the next. Every point of that
geodesic is within `R` of the start, and faces are convex, so every vertex of those faces is within
`R + D`. Walking the faces' boundaries gives a path through the graph to the missing copy that never
goes further than `R + D`. The last vertex along it that the patch still holds is itself within
`R + D`, and it is missing a neighbor: it is incomplete. So if no incomplete vertex is that close,
nothing nearer is missing.

### Count hops exactly

The number of hops to the nearest copy of the target is the word length of
`label(from)⁻¹ · label(to)` in the generators. It is found by breadth-first search of the group
itself (120 elements for S₅), not the tiling, so it is exact however much tiling was generated.
Only the drawn route can run off the patch, in which case it is drawn as far as it goes.

### Break ties consistently

Copies often tie exactly for nearest, and nothing distinguishes them but vertex index, which
recentering permutes. The copy already marked keeps the mark for as long as it is still tied for
nearest, so the marker does not hop between equally good answers.

## Alternatives considered

- **Search around `from` in place, with the same confirmation.** Correct, since it never claims a
  wrong distance, but it confirms too rarely where it matters. A first measurement suggested
  otherwise, but it placed `from` near the middle of the patch, which is not how measurements are
  made. In the viewer, an ordinary pan left every measurement unconfirmed. Across 1521 pairs of
  vertices out to 4.5 from the origin, 1049 could not be confirmed in place; measured around the
  home vertex, all 1521 were.
- **Carry the measurement across recentering unchanged.** Stable, but it freezes whatever the patch
  allowed when the vertices were picked.
- **Search the unit containing `from` and its 20 edge-neighbors.** Tempting, with one neighbor per
  side, but not enough. Unit centers lie 4.83 away (20 units), then 5.78 (14 units). Nearest copies
  were found up to 3.24 away, while a second-ring unit reaches within about 2.9 of the home unit's
  center. The nearest copy was classified as beyond the neighbor ring in 74 of 960 cases. That count
  is approximate, since a vertex on a boundary shared between rings could land in either, but the
  geometry allows such cases either way.
- **Apply the deck transformations to one copy of the target**, so copies need not be in the
  patch. No better in practice: the deck transformations are themselves collected from the q-gons
  in the patch, and this search found a nearer copy than the patch search in none of 960 cases.
- **Generate a larger patch.** Not needed, and it costs memory and load time, especially on the
  web. It remains the lever if confirmation proves too rare for some other tiling.

## Consequences

- Measurements are confirmed wherever the picked vertices are still in the patch, including right
  at its edge. The panning sequence that produced the flicker now reads the same confirmed distance
  on every frame.
- An unconfirmed distance would be shown as "at most", with a note that the pair is too near the
  edge of the tiling to rule out a shorter way.
- Recentering carries only the picked vertices across, not the measurement. A picked vertex carried
  beyond the patch, or onto an unlabelled vertex at its edge, is still dropped, and the measurement
  with it. Since measuring needs only each end's label and position, keeping picks that have left
  the patch would be possible, but is not done yet.
- Tests:
  - `measurements_far_out_are_confirmed`: pairs out to 4.5 from the origin are all confirmed; they
    agree with the in-place search wherever that is confirmed too, the nearest copy sits at the
    reported distance, and hops match the group.
  - `confirmed_distances_agree_wherever_they_are_measured`: moving pairs about the patch, every
    confirmed in-place answer agrees and no unconfirmed one undercuts it.
  - `hop_counts_match_the_word_metric` and `ties_go_to_the_copy_already_marked`.
