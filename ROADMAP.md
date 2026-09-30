# Blockscape Roadmap

Progress tracker for the milestone plan. Each milestone leaves the project
compilable, running and tested.

- [x] **0. Foundation** - cargo project, app loop, fixed timestep, logging,
      input plumbing, display backend, frame pacing, CLI.
- [x] **1. Basic 3D rendering** - perspective camera, view/projection,
      indexed meshes, software rasterizer (clipping, depth, backface cull,
      perspective-correct texturing), directional lighting, sky, FPS/debug
      overlay, interactive fly camera.
- [x] **2. Voxel data model** - block registry with data-driven properties
      (solid/transparent/hardness/textures/light emission), air + first
      materials; no hardcoded block logic.
- [x] **3. Chunks** - efficient chunk storage (`get/set/fill/is_empty`),
      coordinate types (world/chunk/local), negative-coordinate tests,
      boundary tests, configurable dimensions.
- [ ] **4. Voxel meshing** - visible-face meshing, neighbor-aware chunk
      borders, transparent handling.
- [ ] **5. Texture atlas** - atlas builder, per-face UV mapping,
      nearest filtering, bleed-free insets.
- [ ] **6. Procedural terrain** - seeded heightmap terrain: stone/dirt/grass
      column profile, deterministic per (seed, chunk).
- [ ] **7. Biomes** - plains/forest/desert/mountains/taiga; temperature and
      humidity fields; surface + vegetation variation.
- [ ] **8. Caves & ores** - 3D noise carving, tunnels and pockets; coal,
      iron, gold, diamond with configurable distribution.
- [ ] **9. Structures** - rule-based trees, small houses; consistent
      cross-chunk placement without duplicates.
- [ ] **10. World streaming** - render/generation distance config, worker
      threads, priority queues, mesh upload, unloading.
- [ ] **11. Player controller** - AABB collision, gravity, jumping,
      sprinting, ground detection.
- [ ] **12. Raycasting** - DDA voxel traversal: hit block, face, distance,
      placement position.
- [ ] **13. Block interaction** - break/place with per-block hardness,
      targeted mesh rebuilds.
- [ ] **14. Inventory** - 36 + 9 slots, stacking, splitting, hotbar.
- [ ] **15. Crafting** - 2×2 and 3×3 grids, data-driven recipes.
- [ ] **16. Lighting** - sunlight + block light flood fill, torches,
      incremental updates.
- [ ] **17. Day/night** - world clock, sun/moon, sky colors, configurable
      time speed.
- [ ] **18. Water** - source blocks, flow propagation, rendering.
- [ ] **19. Audio** - data-driven sound architecture (native playback when a
      real window backend exists).
- [ ] **20. Mobs** - entity basics, passive + hostile creatures, simple AI,
      damage, drops.
- [ ] **21. Save system** - versioned saves: seed + generator version +
      modified chunks, player and entity state.
- [ ] **22. Performance pass** - measurements, then targeted optimization
      (parallel meshing, caching, culling).
- [ ] **23. Greedy meshing** - merged faces, benchmarks vs. naive mesher.
- [ ] **24. Debug tools** - on-screen overlays, chunk borders, hitboxes,
      raycast visualization.
- [ ] **25. Modding architecture** - clean registries for blocks/items/
      recipes/biomes/structures/entities/sounds; data-driven definitions.
- [ ] **26. Multiplayer foundation** - authoritative server, chunk/entity
      synchronization.

## Dependency plan

Added only when their milestone lands:

| Milestone | Crate(s)                                   | Status |
| --------- | ------------------------------------------ | ------ |
| now       | `glam`                                      | in use |
| 3, 6, 8   | `fastnoise-lite` (noise)                    | vendored locally for the cloud build env |
| 10        | `rayon` (or hand-rolled pool)               | planned |
| 21        | `serde` + `bincode`                         | planned |
| GPU window| `wgpu`, `winit`, `image`, `bytemuck`        | planned (behind existing seams) |
| errors/log| `anyhow`, `thiserror`, `tracing`            | evaluated at each integration point |

Note: the primary development sandbox cannot reach `crates.io` (network
policy); `glam` and `fastnoise-lite` are vendored from their GitHub
repositories into a local directory source there. This does not change the
project's manifest: on a normal machine, `cargo build` fetches the same
crates from crates.io normally.
