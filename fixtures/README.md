# Fixtures

Tiny standalone Metal programs, each producing textures with exact ground
truth, plus the scripts that capture them. Copied from the
`gputools-replay` campaign on 2026-09-02 and maintained here so this repo
is free-standing. `fixtures/build-all.sh` builds and captures all of them
into `captures/` (gitignored).

Every app is two-phase: it creates its resources, blocks on a go-file,
then runs a final command inside the capture. `capture-late.sh` starts
`gpucapture` during the block, so the resources pre-exist the capture
boundary (a resource created and destroyed inside one capture is not
snapshotted for fetch). `capture.sh` is the single-phase variant, kept for
`known-textures`' non-late mode.

Textures that no captured command reads answer a fetch only under
`--force-load-unused` (`MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1`).

| app | what it makes | ground truth | needs force-load |
| --- | --- | --- | --- |
| `known-textures.m` | 7 BGRA8Unorm textures, one distinct width each (64, 80, ...) | blit source (w=64) fully cyan `00 ff ff ff` BGRA; blit destination (w=80) cyan in its 64x64 region | yes |
| `known-depth.m` | full-screen triangle at depth 0.5 into Depth32Float, blit-stored | the stored texture reads 0.5 everywhere; the other endpoint is uninitialised | no |
| `known-depth-stencil.m` | one combined Depth32Float_Stencil8 resource, rendered at depth 0.5 with stencil 42, blit-stored | depth aspect reads 0.5, stencil aspect reads 42, both from one streamRef (hl live_hl_aspects) | no |
| `known-stencil.m` | base Stencil8 (42) and a combined DS with an X32_Stencil8 view | the base Stencil8 reads 42; combined aspects fetch separately | no |
| `known-astc.m` | 64x64 ASTC_4x4_LDR filled with one 16-byte block pattern | raw blocks `00..0f` repeated 256 times, 4096 bytes | no |
| `known-ycbcr.m` (needs CoreVideo) | 64x64 biplanar 4:2:0 CVPixelBuffer wrapped as two textures | luma R8Unorm 64x64 all 128; chroma RG8Unorm 32x32 all (100, 150) | no |
| `known-ambiguous.m` | three 64x64 BGRA8Unorm textures, same geometry | red = 1 mip, green = 3 mips, blue = 7 mips (pixel colour pins the descriptor's mip count) | yes |
| `known-3d.m` | 16x16x4 BGRA8Unorm volume, z-slices distinct | the fetch serves one z-plane and reports depth 1; the descriptor says Type3D depth 4 | yes |
| `known-mips.m` | 2-slice 2D array, 7-level chain, red/green | slice 0 red, slice 1 green; out-of-range level/slice CLAMPS (for the mip/slice follow-up) | yes |

Latency and hygiene for anything that then reads these captures through
the replayer: see the top-level README.
