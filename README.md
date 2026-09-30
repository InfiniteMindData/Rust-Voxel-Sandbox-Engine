# Blockscape

An original voxel sandbox game and engine written in Rust. Blockscape is built
incrementally, milestone by milestone, towards: procedural worlds, chunk
streaming, destructible terrain, inventory and crafting, lighting, biomes,
caves, structures, creatures, day/night, persistent worlds and - later -
multiplayer and modding. See [ROADMAP.md](ROADMAP.md) for progress and
[ARCHITECTURE.md](ARCHITECTURE.md) for how it fits together.

## Status

**Milestones 0-1 complete**: the engine boots, renders a lit, textured,
tumbling cube with correct perspective and depth testing, streams frames to
your browser and accepts full first-person fly-camera input (WASD, mouse
look, sprint). 39 unit tests pass; the build is warning-free.

## Why a browser display?

The primary development environment is a headless Linux sandbox (no display
server, no GPU), so the engine ships with a zero-dependency display backend:
frames are encoded to PNG by a hand-written DEFLATE/PNG encoder and streamed
to a browser over HTTP, which also feeds keyboard/mouse input back. The game
logic targets a small [`Renderer`] trait and an [`InputState`] abstraction, so
a native `winit` + `wgpu` window backend can be added behind the same seams
without touching gameplay code (this is the plan once `crates.io` is
reachable from the build environment).

## Build

Rust stable (1.75+ recommended).

```sh
cargo build --release
```

## Run

```sh
cargo run --release -- --port 8080
```

Then open `http://localhost:8080` in a browser.

Options:

```text
--port <n>       display server port (default 8080)
--width <n>      render width in pixels (default 480)
--height <n>     render height in pixels (default 270)
--seed <n>       world seed (used by terrain generation, milestone 6)
-h, --help       show help
```

Environment: `BLOCKSCAPE_LOG` = `error` | `warn` | `info` | `debug` | `trace`.

## Controls

| Input          | Action                                   |
| -------------- | ---------------------------------------- |
| Click          | Capture mouse (pointer lock)             |
| Mouse          | Look                                     |
| `W` `A` `S` `D`| Move                                     |
| `Space`        | Move up                                  |
| `Ctrl`         | Move down                                |
| `Shift`        | Sprint                                   |
| `F3`           | Toggle debug stats overlay               |

## Dependencies

Deliberately minimal:

- `glam` - SIMD-friendly 3D math.

Everything else (logging, PNG/DEFLATE encoding, HTTP display server, input,
rasterization) is implemented in-tree with the standard library. The full
target stack for later milestones (`wgpu`, `winit`, `serde`, `rayon`,
`fastnoise-lite`, `anyhow`, `thiserror`, `tracing`, `bytemuck`) is documented
in [ROADMAP.md](ROADMAP.md); each dependency is added only when its system
milestone is actually implemented.

## Development

```sh
cargo test          # unit tests for every system
cargo check         # fast type check
```

### Project layout

```text
src/
├── main.rs            entry point, CLI
├── engine/            app loop, display, input, camera, timing, logging
├── render/            framebuffer, rasterizer, textures, PNG encoder
└── voxel/             (milestone 2+) blocks, chunks, world data
assets/ui/index.html   the browser-side display page
worlds/                saved worlds (runtime-created)
```

## License

MIT - see [LICENSE](LICENSE).
