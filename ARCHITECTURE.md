# Blockscape Architecture

This document describes how the engine works today and the seams it is
designed around. It is updated as milestones land.

## Design principles

- **Small, verifiable milestones.** The project must compile, run and pass
  tests after every milestone (see `ROADMAP.md`).
- **Data-oriented voxel storage.** No heap object per block; chunks are flat
  typed arrays of block IDs (milestone 3+).
- **Systems separated by responsibility.** Simulation never touches the
  display; rendering never mutates world state.
- **Backend seams from day one.** Rendering and windowing live behind traits
  so the software/display-server backend used in the cloud development
  environment can be replaced by `wgpu`/`winit` without gameplay changes.
- **Dependencies are earned.** Every external crate must map to a concrete
  milestone need.

## Layering

```text
┌──────────────────────────────────────────────────────────────┐
│ main.rs         CLI parsing, logging init, app bootstrap     │
├──────────────────────────────────────────────────────────────┤
│ engine::app     fixed-timestep loop: input → simulate →      │
│                 render → publish; owns all subsystems        │
├──────────────┬───────────────────────┬───────────────────────┤
│ engine::input│ engine::camera        │ engine::renderer      │
│ InputState   │ yaw/pitch camera,     │ Renderer trait +      │
│ (backend-fed)│ view/projection       │ SoftwareRenderer      │
├──────────────┴───────────────────────┴───────────────────────┤
│ engine::window  HttpDisplay: PNG frame slot, input POSTs,    │
│                 HUD stats - the "window system" for now      │
├──────────────────────────────────────────────────────────────┤
│ render::        framebuffer, rasterizer (clip → screen →     │
│                 pixels), textures, mesh, png (DEFLATE)       │
└──────────────────────────────────────────────────────────────┘
```

Arrows point downwards only: `render` knows nothing about the game;
`engine` knows nothing about PNG bytes; the browser knows nothing about Rust.

## Threading model

- **Main thread**: simulation (fixed 60 Hz) and rendering (30 fps cap).
- **display-accept thread**: accepts TCP connections, spawns one
- **display-conn thread per client**: serves HTML, `/stream` (multipart
  PNG), `/frame.png`, `/stats`, and ingests `POST /input`.

Shared state crosses threads through three explicitly owned channels:

1. `FrameSlot` - mutex + condvar double-hand-off of the latest rendered
   frame (sequence-numbered; streamers block until a newer seq appears).
2. `Mutex<InputState>` - browser events accumulate; the simulation drains
   mouse deltas and button queues once per step.
3. `Mutex<EngineStats>` - snapshot for the HUD, written once per frame.

No background thread ever touches the renderer or framebuffer.

## Rendering pipeline (software backend)

Per frame:

1. **begin_frame** - clear, then draw the sky as two untextured triangles
   with depth-test disabled (a backdrop, not geometry).
2. **draw_mesh** - for each visible instance:
   - model transform + Lambert shading (`ambient + diffuse·max(N·L,0)`)
     computed per vertex on the CPU;
   - clip-space transform by the camera view-projection matrix;
   - Sutherland-Hodgman near-plane clip (`w ≥ ε`);
   - perspective divide, viewport map, backface cull (CCW front);
   - edge-function rasterization with perspective-correct UV/color
     interpolation (the `1/w` trick), strict-LESS z-buffer.
3. **end_frame → publish** - the finished RGB buffer is handed to the
   display backend, encoded as PNG (see below) and streamed.

The `Renderer` trait mirrors the data flow a GPU needs (upload once, draw
per frame with a model matrix), so a `wgpu` implementation is a new backend,
not a rewrite. Conventions match WebGPU: right-handed views, depth range
`0..1`, CCW front faces.

## PNG/DEFLATE encoder

`render::png` implements exactly what the display pipeline needs:

- greedy LZ77 with hash chains (32 KiB window) emitting **fixed-Huffman**
  DEFLATE blocks (RFC 1951 §3.2.6);
- zlib wrapper with adler32 (RFC 1950);
- PNG chunks (IHDR/IDAT/IEND) with CRC32, 8-bit RGB, per-row **Up** filter.

Correctness is pinned by tests: CRC/adler known vectors, symbol-table range
checks, and bitstream round-trips through a test-only inflater. The output
has also been cross-validated against the reference `zlib` decoder.

## Camera

`engine::camera::Camera` composes a yaw/pitch orientation into view and
perspective matrices (`glam`'s right-handed `look_to` / `perspective`,
depth `0..1`). Pitch is clamped to ±89°. Unit tests project known world
points and assert their NDC positions.

## Timing

`FixedTimestep` runs simulation at a fixed rate inside the free-running
render loop (accumulator pattern with a catch-up clamp so a stalled frame
cannot spiral). `FpsCounter` keeps an exponential moving average for the
HUD. Render frames are paced by a deadline (default 30 fps cap); simulation
steps are independent of frame rate.

## Display protocol

| Route           | Purpose                                        |
| --------------- | ---------------------------------------------- |
| `GET /`         | the UI page (`assets/ui/index.html`)           |
| `GET /frame.png`| single latest frame; the UI polls this as the frame transport (works through iframes/proxies that break multipart streams) |
| `GET /stream`   | `multipart/x-mixed-replace` PNG stream (alternative transport, `X-Accel-Buffering: no`) |
| `GET /stats`    | HUD text (fps, frame time, pos, counters)      |
| `POST /input`   | form-encoded events: `ke=Code:1,…`, `mb=0:1,…`, `mdx=`, `mdy=` |

The browser page renders the multipart stream in a plain `<img>` element
(no client-side decoding), batches input events on a 30 ms timer, and uses
pointer lock for mouse look with drag-look as fallback.

## Error handling

`engine::error::{EngineError, Result}` covers application-level failures
(I/O, config). Internal invariants use `debug_assert!` (rasterizer index
bounds, buffer sizes); recoverable subsystems will grow typed errors via
`thiserror` when their milestones land. `unwrap()` is allowed only where the
invariant is genuinely guaranteed (mutex poisoning recovery uses
`into_inner()` instead).

## Testing

39 unit tests currently cover: logging levels, fixed timestep (including
catch-up clamping), FPS convergence, camera projection (centering, yaw
basis, aspect, pitch clamp), input mapping/accumulation/queuing, framebuffer
invariants, cube mesh topology/normals, DEFLATE/PNG round-trips, and
rasterizer behavior (full-quad coverage, culling, z-buffering, near clipping,
texture mapping, off-screen safety).

## Planned evolution

The next architectural additions (tracked in `ROADMAP.md`):

- `voxel::` - block registry, chunks (16×16×384 default, configurable),
  world type, coordinate types with exhaustive conversion tests.
- Worker-thread generation + meshing with job queues feeding the renderer
  through message passing (no GPU access off the main thread).
- `wgpu` backend behind `Renderer`; `winit` backend behind the same
  input/display contracts.
- Save system: seed + generator version + modified-chunk deltas, versioned
  binary format.
