// A fixture app with exact ground truth: N textures, each filled with a
// distinct solid colour, and nothing else.
//
// Purpose. The fetch coverage gap (dossier 00) is measured today as "180 of
// 2001 swept streamRefs answered" on a capture nobody controls, which cannot
// say whether the 1821 silent refs are resources that exist and refuse to
// answer, or simply refs that were never resources. This app removes that
// ambiguity: it creates a known, small, enumerated set of textures, so a
// sweep of its capture has a ground truth to be compared against.
//
// It also gives the first correctness check for playback: each texture is
// cleared to one exact colour, so a fetched payload can be checked pixel for
// pixel rather than merely counted.
//
// Every texture is filled by a render pass whose load action is Clear and
// whose encoder does no drawing, so the contents are exactly the clear colour
// with no shader, pipeline state, or vertex data involved.
//
// Two-phase mode. If KNOWN_TEXTURES_GO_FILE is set, the app creates and fills
// the six textures (phase 1), then blocks until that file exists, then runs
// phase 2: it creates a SEVENTH texture and clears it, and re-runs the blit.
// A capture started during the wait therefore contains six resources that
// pre-existed its boundary and one created inside it, in one trace. That is
// the experiment for whether fetch serves a capture-time snapshot: see
// fixtures/capture-late.sh and docs/findings/00-texture-fetch.md.
//
// Build (no Xcode project, per fixtures/README.md):
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-textures \
//         fixtures/known-textures.m \
//         -framework Metal -framework Foundation
//
// Capture:
//   fixtures/capture.sh /tmp/known-textures captures/known-textures.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <unistd.h>

// The ground truth. Each entry is one texture: a label, its dimensions, its
// pixel format, and the RGBA clear colour it is filled with. Keep this table
// in sync with fixtures/README.md - a probe checks fetched payloads
// against these values.
typedef enum {
    FillClear,      // written by a render pass Clear load action
    FillBlitDst,    // written by a blit copy from another texture
    FillCPUUpload,  // written by -replaceRegion:, no GPU work at all
} FillKind;

// The ground truth AND the experiment. Each row varies ONE property from the
// baseline row so that, when a sweep of the capture answers some refs and not
// others, the answering set names the property that matters. Widths are all
// distinct so a reply record identifies its row by width alone.
typedef struct {
    const char *label;
    NSUInteger width;      // distinct per row: the row's identity in a reply
    NSUInteger height;
    MTLPixelFormat format;
    MTLStorageMode storage;
    MTLTextureUsage usage;
    FillKind fill;
    double r, g, b, a;
} FixtureTexture;

static const FixtureTexture kTextures[] = {
    // Baseline: exactly what the first version of this app made, and which
    // answered no fetch at all.
    {"private_rt_read",  16, 16, MTLPixelFormatBGRA8Unorm, MTLStorageModePrivate,
     MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead, FillClear, 1.0, 0.0, 0.0, 1.0},
    // Vary storage mode only.
    {"shared_rt_read",   32, 32, MTLPixelFormatBGRA8Unorm, MTLStorageModeShared,
     MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead, FillClear, 0.0, 1.0, 0.0, 1.0},
    // Vary usage only (no ShaderRead: a pure render target).
    {"private_rt_only",  48, 48, MTLPixelFormatBGRA8Unorm, MTLStorageModePrivate,
     MTLTextureUsageRenderTarget, FillClear, 0.0, 0.0, 1.0, 1.0},
    // Vary "is it ever read": this one is the source of a blit, so something
    // downstream consumes it.
    {"private_blit_src", 64, 64, MTLPixelFormatBGRA8Unorm, MTLStorageModePrivate,
     MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead, FillClear, 1.0, 1.0, 0.0, 1.0},
    // Vary how it is written: by a blit rather than a clear.
    {"private_blit_dst", 80, 64, MTLPixelFormatBGRA8Unorm, MTLStorageModePrivate,
     MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead, FillBlitDst, 0.0, 0.0, 0.0, 1.0},
    // Vary it entirely: contents uploaded from the CPU, never touched by the GPU.
    {"shared_cpu_upload", 96, 96, MTLPixelFormatBGRA8Unorm, MTLStorageModeShared,
     MTLTextureUsageShaderRead, FillCPUUpload, 1.0, 0.0, 1.0, 1.0},
};
static const size_t kTextureCount = sizeof(kTextures) / sizeof(kTextures[0]);

// Created only in phase 2 of two-phase mode, i.e. INSIDE the capture. Same
// properties as the baseline row so the only difference is when it was made.
static const FixtureTexture kLateTexture =
    {"late_created", 112, 112, MTLPixelFormatBGRA8Unorm, MTLStorageModePrivate,
     MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead, FillClear, 0.0, 1.0, 1.0, 1.0};

// Index of the blit source and destination rows, by label order above.
static const size_t kBlitSrc = 3;
static const size_t kBlitDst = 4;

static id<MTLTexture> makeTexture(id<MTLDevice> device, const FixtureTexture *t) {
    MTLTextureDescriptor *desc = [MTLTextureDescriptor
        texture2DDescriptorWithPixelFormat:t->format
                                     width:t->width
                                    height:t->height
                                 mipmapped:NO];
    desc.usage = t->usage;
    desc.storageMode = t->storage;
    id<MTLTexture> tex = [device newTextureWithDescriptor:desc];
    tex.label = [NSString stringWithUTF8String:t->label];
    return tex;
}

static void encodeClear(id<MTLCommandBuffer> cb, id<MTLTexture> tex, const FixtureTexture *t) {
    MTLRenderPassDescriptor *pass = [MTLRenderPassDescriptor renderPassDescriptor];
    pass.colorAttachments[0].texture = tex;
    pass.colorAttachments[0].loadAction = MTLLoadActionClear;
    pass.colorAttachments[0].storeAction = MTLStoreActionStore;
    pass.colorAttachments[0].clearColor = MTLClearColorMake(t->r, t->g, t->b, t->a);
    id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:pass];
    enc.label = [NSString stringWithFormat:@"clear_%s", t->label];
    [enc endEncoding];
}

static void encodeBlit(id<MTLCommandBuffer> cb, id<MTLTexture> src, id<MTLTexture> dst) {
    id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
    blit.label = @"blit_src_to_dst";
    [blit copyFromTexture:src
              sourceSlice:0
              sourceLevel:0
             sourceOrigin:MTLOriginMake(0, 0, 0)
               sourceSize:MTLSizeMake(src.width, src.height, 1)
                toTexture:dst
         destinationSlice:0
         destinationLevel:0
        destinationOrigin:MTLOriginMake(0, 0, 0)];
    [blit endEncoding];
}

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) {
            fprintf(stderr, "known-textures: no Metal device\n");
            return 1;
        }
        printf("device: %s\n", device.name.UTF8String);

        id<MTLCommandQueue> queue = [device newCommandQueue];
        if (!queue) {
            fprintf(stderr, "known-textures: no command queue\n");
            return 1;
        }

        NSMutableArray<id<MTLTexture>> *textures = [NSMutableArray array];
        for (size_t i = 0; i < kTextureCount; i++) {
            const FixtureTexture *t = &kTextures[i];
            id<MTLTexture> tex = makeTexture(device, t);
            if (!tex) {
                fprintf(stderr, "known-textures: texture %s failed\n", t->label);
                return 1;
            }
            [textures addObject:tex];
        }

        // CPU uploads first: no command buffer involved, so if one of these
        // answers a fetch it proves the replayer can serve a resource the GPU
        // never wrote.
        for (size_t i = 0; i < kTextureCount; i++) {
            const FixtureTexture *t = &kTextures[i];
            if (t->fill != FillCPUUpload) continue;
            size_t bpr = t->width * 4;
            uint8_t *px = malloc(bpr * t->height);
            // BGRA byte order, matching the row's declared colour.
            uint8_t b = (uint8_t)(t->b * 255), g = (uint8_t)(t->g * 255);
            uint8_t r = (uint8_t)(t->r * 255), a = (uint8_t)(t->a * 255);
            for (size_t p = 0; p < bpr * t->height; p += 4) {
                px[p] = b; px[p + 1] = g; px[p + 2] = r; px[p + 3] = a;
            }
            [textures[i] replaceRegion:MTLRegionMake2D(0, 0, t->width, t->height)
                           mipmapLevel:0
                             withBytes:px
                           bytesPerRow:bpr];
            free(px);
        }

        // Phase 1: one command buffer, a clear pass per Clear row, then one blit
        // that makes the source row a resource something actually reads.
        id<MTLCommandBuffer> cb = [queue commandBuffer];
        for (size_t i = 0; i < kTextureCount; i++) {
            if (kTextures[i].fill == FillClear) encodeClear(cb, textures[i], &kTextures[i]);
        }
        encodeBlit(cb, textures[kBlitSrc], textures[kBlitDst]);
        [cb commit];
        [cb waitUntilCompleted];

        if (cb.error) {
            fprintf(stderr, "known-textures: command buffer error: %s\n",
                    cb.error.localizedDescription.UTF8String);
            return 1;
        }

        // Phase 2 (two-phase mode only): wait for the go-file, then create one
        // more texture and do more GPU work, all inside the capture.
        const char *goFile = getenv("KNOWN_TEXTURES_GO_FILE");
        if (goFile && *goFile) {
            printf("phase 1 done; waiting for go-file %s\n", goFile);
            fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) {
                usleep(100000);
                if (++waited > 600) {
                    fprintf(stderr, "known-textures: go-file never appeared\n");
                    return 1;
                }
            }
            id<MTLTexture> late = makeTexture(device, &kLateTexture);
            if (!late) {
                fprintf(stderr, "known-textures: late texture failed\n");
                return 1;
            }
            id<MTLCommandBuffer> cb2 = [queue commandBuffer];
            encodeClear(cb2, late, &kLateTexture);
            encodeBlit(cb2, textures[kBlitSrc], textures[kBlitDst]);
            [cb2 commit];
            [cb2 waitUntilCompleted];
            if (cb2.error) {
                fprintf(stderr, "known-textures: phase 2 error: %s\n",
                        cb2.error.localizedDescription.UTF8String);
                return 1;
            }
            printf("phase 2 done: created and cleared %s (w=%lu) inside the capture\n",
                   kLateTexture.label, (unsigned long)kLateTexture.width);
        }

        printf("filled %zu textures:\n", kTextureCount);
        for (size_t i = 0; i < kTextureCount; i++) {
            const FixtureTexture *t = &kTextures[i];
            static const char *kFill[] = {"clear", "blit-dst", "cpu-upload"};
            printf("  %-18s w=%-4lu %4lux%-4lu fmt %2lu storage %lu usage %lu fill %-10s "
                   "rgba(%.2f, %.2f, %.2f, %.2f)\n",
                   t->label, (unsigned long)t->width, (unsigned long)t->width,
                   (unsigned long)t->height, (unsigned long)t->format,
                   (unsigned long)t->storage, (unsigned long)t->usage,
                   kFill[t->fill], t->r, t->g, t->b, t->a);
        }
        printf("done\n");
    }
    return 0;
}
