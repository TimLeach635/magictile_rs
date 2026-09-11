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
| UI / windowing | `eframe` 0.34 (wgpu backend; 0.35 can't resolve `egui_glow`) |
| GPU rendering | `wgpu` (re-exported by eframe, via an `egui-wgpu` paint callback) |
| Vertex data | `bytemuck` |
| XML reading | `roxmltree` (writing is hand-rolled to match DataContract output) |
| Embedded config dir | `include_dir` |
| Parallel puzzle building | `rayon` (optional feature; results are order-independent) |
| Randomness | `rand` |
| Headless screenshots | `pollster`, `png` |
| Logging (`RUST_LOG=wgpu=warn`, …) | `env_logger` |

Complex numbers, matrices and thick lines are small enough to live in `r3` / the renderer, which keeps
.NET numeric behaviour under our control. Still to add in phase 4: `rfd` (file dialogs), `directories`
(settings location).

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
  `MarkCellsForStateCalcs`; null deref in `Loader.SetVersionOnConfig` when `Version` is missing;
  the off colour (-1) of lights-on puzzles saved as `0ffffffff`, which couldn't be loaded (we write
  `ff` and read both); infinite loop when group relations can't generate enough identifications;
  loading macros from a saved log (the original crashed).
- **DataContract reading semantics are reproduced** (`magictile_core::xml::data_contract_members`):
  elements are matched in alphabetical member order and out-of-order ones are ignored, and missing
  values are zero/empty (the deserializer skips constructors, e.g. `UseMirroredEdgeSet` defaults to
  false in files but true in code). Two shipped files depend on this.
- **C# object aliasing is reproduced** where it changes results: a polygon reflected in one of its own
  segments (`Polygon::reflect_in_own_segment`), and `Pants.Clone` not copying its isometry.
- Unused R3 code is not ported: Honeycombs, Shapeways, PovRay, STL, GraphRelaxation, Golden, VRML,
  Polytope, Surface/Torus/KleinBottle, RotationHandler4D, Matrix4D, Lighting, VBO.

## Phases

1. ✅ **`r3` geometry** — tolerance, `Vector3D`, infinity handling, Möbius, isometry, circles (incl. `CircleNE`),
   segments / polygons, 2D geometry helpers (Euclidean / spherical / hyperbolic), tilings, slicer
   (incl. hyperbolic equidistant offsets for systolic puzzles), near tree, hyperbolic & spherical model
   maps, texture-coordinate helpers, .NET-compatible hashing. 50 unit tests.
   Known original behaviour kept: spherical tilings rely on an exact `NumTiles` (the face at infinity
   can't be deduplicated), and the slicer can't cut a circle lying entirely inside a polygon.
2. ✅ **`magictile-core`** — config + menu loading (configs embedded), puzzle building, topology
   analysis, state, twisting (`TwistController`, driven per frame), history, macros, setup moves /
   commutators, lights-on toggling, save/load of logs and macro files.
   All 2287 library entries build (`cargo run --release -p magictile-core --example build_stats`;
   also `cargo test --release -- --ignored`). Colour counts match `ExpectedNumColors` except two
   classes whose files disagree with themselves in the original too ({4,4} 9C (shift), {6,3} 9C (3x3));
   the latter's author comment ("setting 9 only loads 7 colours") is reproduced exactly.
   Per-cell texture vertices are computed by the renderer (`scene::RenderData`); the hemisphere-disks
   model is drawn directly, so `PrepareSurfaceData` isn't needed.
   Slowest builds (~5 s, e.g. {8,4} 5C F0:0.85:0 E0.5:0:0) spend their time marking affected stickers;
   a spatial prefilter could speed this up without changing results.
3. ✅ **Rendering and interaction** (`crates/magictile`) — CPU geometry each frame into draw lists,
   drawn by wgpu into an offscreen 4x MSAA target and blitted into egui.
   - Spherical puzzles are drawn sticker by sticker with a two-pass stencil fill (concave and
     "inverted" polygons containing infinity); the hemisphere-disks model uses a stencil clip bit.
   - Euclidean / hyperbolic puzzles render each master cell into a layer of a 512² texture array
     (with mipmaps) and map it onto every visible copy, with the original's level-of-detail rules.
   - All display models: Poincaré, Klein, upper half-plane, orthographic; stereographic, gnomonic,
     fisheye, hemisphere disks (F7 cycles).
   - The original's `MouseMotion`: pan (Don Hatch's pure hyperbolic translation), rotate (right
     drag), zoom (middle drag / scroll), gliding after a flick, recentering onto the nearest copy.
   - Twisting-circle highlighting (incl. systolic hypercycles / pants and earthquake segments),
     left / right click twists with eased animation, held number keys for slice masks, Alt-click
     macros (Ctrl+Alt+click records), lights-on toggling, undo / redo, scrambles.
   - A minimal menu bar and puzzle tree (phase 4 fleshes these out).
   - Testing without a person: `magictile --screenshot out.png [options] [puzzle]` renders headless;
     `MAGICTILE_SELFTEST` / `MAGICTILE_SCRIPT` drive the real app with scripted input and capture
     frames (see `selftest.rs`; it submits its own GPU work so it works even when the window is
     occluded and egui skips presenting).
   Deliberate differences: scroll-wheel zoom follows the platform convention; gliding stops when the
   speed (not either component) is small. Kept original behaviour: earthquake twists show small gaps
   mid-animation (the original draws them the same way).
4. **App shell** — menus, puzzle tree, settings panel, macro list, status bar, dialogs, keyboard
   shortcuts, solved notification.
5. **Extras (optional)** — GAP export, SVG export, screenshots.

## Deferred

- Reference data from the original app (per-puzzle cell / sticker / twist counts and twist-order
  checksums) to verify build parity. Needs a Windows machine; until then we verify colour counts and
  load sample save files.
