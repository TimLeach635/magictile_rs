# MagicTile → Rust port plan

Port of [MagicTile](http://www.roice3.org/magictile) (C#, WinForms + OpenTK) to cross-platform Rust.
The original source lives in `MagicTile/` for reference.

## Decisions

| Topic | Decision |
|---|---|
| Scope | **2D puzzles only.** No surface display (sphere / Boy's / Clifford torus / Klein bottle), IRP, 4D skew polyhedra, lighting or stereo. |
| 2D models kept | Hyperbolic: Poincaré, Klein, upper half-plane, orthographic. Spherical: stereographic, gnomonic, fisheye, hemisphere disks. |
| Save files | **Read and write** the original `MagicTileLog` XML (config + state + history + macros). Old (2.0 "preview") twist-data layout supported for loading. |
| Stack | `eframe`/`egui` + `wgpu`. |
| Targets | Desktop now (macOS / Windows / Linux). Keep wasm possible: core crates avoid threads and file I/O. |
| Settings | App-local, new format (serde) in the platform config dir. Not compatible with the WinForms settings. |

## Workspace layout

```
crates/
  r3/               pure geometry (no GUI deps)
  magictile-core/   config, puzzle build, state, twisting, history, macros, persistence (headless)
  magictile/        eframe + wgpu application
MagicTile/          original C# source (reference only)
```

## Dependencies

| Need | Crate |
|---|---|
| Complex numbers (Möbius maps) | `num-complex` |
| Camera / projection matrices | `glam` |
| GPU rendering | `wgpu` (via `egui-wgpu` callback) |
| UI / windowing | `eframe`, `egui`, `egui_extras` |
| Thick lines (twist circles) | `lyon` (stroke tessellation) |
| XML | `quick-xml` (+ `serde`) |
| Embedded config dir | `include_dir` |
| File dialogs | `rfd` (async API, works on wasm) |
| Parallelism | `rayon` (must stay deterministic) |
| Randomness | `rand` |
| Platform dirs | `directories` |
| Screenshots | `image` |

## Porting notes / invariants

- **Deterministic build order is a correctness requirement.** Saved history and macros reference
  twist-data indices, so cells, twist data and stickers must be produced in the same order as the C# code.
  Make every parallel step order-independent.
- **Hashed collections emulate .NET exactly** (`r3::nethash`: `NetMap`, `NetSet`, `distinct`). The C# uses
  vectors, circles, Möbius maps and polygons as keys, with tolerance-based `Equals` (1e-6) and hashes
  built from rounded coordinates XORed together. Those aren't consistent (e.g. every point with x == y
  hashes alike, so diagonal points match purely by tolerance), so we reproduce .NET's rules: 31-bit hash
  match + `stored.Equals(probe)`, newest-first bucket search, insertion-order enumeration. Use these
  wherever the C# used `Dictionary`/`HashSet`/`Distinct` on geometric keys.
- **`r3::Complex` mirrors `System.Numerics.Complex`** (Smith division, `Reciprocal(0) == 0`, scalars
  promoted to complex so signed zeros/NaNs match). When porting, write `z * -1.0` where C# wrote
  `z * -1`, not `-z`.
- **C# struct-property quirks are preserved**: calls like `Center.Empty()` on an auto-property mutate a
  temporary copy (no-op), and `Circle.From2Points` normalizes only if the circle was already a line.
- Use `r3::util::stable_sort_by` for tolerance comparators (`slice::sort_by` may panic on non-total
  orders) and `r3::util::sum` for float sums (`Sum` starts from -0.0).
- **Index-based data model** instead of the C# reference graph: cells, stickers, twist data and
  identified-twist-data live in `Vec`s addressed by typed ids.
- Unused config sections (`IRPConfig`, `SurfaceConfig`, `Skew4DConfig`) are kept in the data model so
  they round-trip through save files.
- The four view-only IRP configs (no identifications) are hidden from the menu.
- Known C# bugs to fix rather than port: data race on `List.Add` inside `Parallel.For` in
  `MarkCellsForStateCalcs`; null deref in `Loader.SetVersionOnConfig` when `Version` is missing.
- Unused R3 code is not ported: Honeycombs, Shapeways, PovRay, STL, GraphRelaxation, Golden, VRML,
  Polytope, Surface/Torus/KleinBottle, RotationHandler4D, Matrix4D, Lighting, VBO.

## Phases

1. ✅ **`r3` geometry** — tolerance, `Vector3D`, infinity handling, Möbius, isometry, circles (incl. `CircleNE`),
   segments / polygons, 2D geometry helpers (Euclidean / spherical / hyperbolic), tilings, slicer
   (incl. hyperbolic equidistant offsets for systolic puzzles), near tree, hyperbolic & spherical model
   maps, texture-coordinate helpers, .NET-compatible hashing. 50 unit tests.
   Known original behaviour kept: spherical tilings rely on an exact `NumTiles` (the face at infinity
   can't be deduplicated), and the slicer can't cut a circle lying entirely inside a polygon.
2. **`magictile-core`** — config + menu loading, puzzle building, topology analysis, state, twisting,
   history, macros, setup moves / commutators, lights-on toggling, save/load. Headless test that builds
   every shipped puzzle config and checks colour counts against `ExpectedNumColors`.
3. **Rendering** — direct spherical rendering (stencil fill for concave / inverted polygons),
   render-to-texture for Euclidean / hyperbolic cells, model options, pan / rotate / zoom with gliding,
   twisting-circle highlighting, twist animation.
4. **App shell** — menus, puzzle tree, settings panel, macro list, status bar, dialogs, keyboard
   shortcuts, solved notification.
5. **Extras (optional)** — GAP export, SVG export, screenshots.

## Deferred

- Reference data from the original app (per-puzzle cell / sticker / twist counts and twist-order
  checksums) to verify build parity. Needs a Windows machine; until then we verify colour counts and
  load sample save files.
