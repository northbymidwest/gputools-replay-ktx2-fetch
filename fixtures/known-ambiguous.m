// A fixture to RESOLVE the intra-dims-run ordering question for the bundle
// descriptor join (docs/findings/00-texture-fetch.md, the ordering bridge).
//
// The bridge says fetch-streamRefs-ascending correspond rank-for-rank with
// bundle descriptors sorted by store0 offset. Validated ACROSS dims runs, but
// WITHIN a run of identical-dims textures nothing measured proves which
// physical texture a rank maps to (the fetched base bytes look alike when the
// content is alike). This fixture makes them NOT alike: three 64x64 BGRA8
// textures sharing dims+format but differing in BOTH mip count AND solid
// colour, so the fetched pixels identify each physical texture and the
// construction pins colour->mip. The spike then checks whether the store0
// offset rank predicts the colour/mip, i.e. whether the rank-zip is correct
// inside a run.
//
//   red   -> 1 mip level
//   green -> 3 mip levels
//   blue  -> 7 mip levels
//
// Textures are never used (force-load path), captured with a late boundary so
// they pre-exist the capture with their level-0 content.
//
// Build:
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-ambiguous \
//         fixtures/known-ambiguous.m -framework Metal -framework Foundation
// Capture:
//   fixtures/capture-late.sh /tmp/known-ambiguous captures/known-ambiguous.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static id<MTLTexture> make(id<MTLDevice> device, NSUInteger mips, const char *label) {
    const NSUInteger W = 64, H = 64;
    MTLTextureDescriptor *td = [[MTLTextureDescriptor alloc] init];
    td.textureType = MTLTextureType2D;
    td.pixelFormat = MTLPixelFormatBGRA8Unorm;
    td.width = W; td.height = H; td.arrayLength = 1;
    td.mipmapLevelCount = mips;
    td.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
    td.storageMode = MTLStorageModeShared;
    id<MTLTexture> t = [device newTextureWithDescriptor:td];
    t.label = [NSString stringWithUTF8String:label];
    return t;
}

// Clear texture level 0 to (r,g,b) via a render pass, so the texture is
// referenced by the command stream (hence captured) and its level-0 content
// is the identifying colour.
static void clear_to(id<MTLCommandQueue> queue, id<MTLTexture> t,
                     double r, double g, double b) {
    MTLRenderPassDescriptor *pass = [MTLRenderPassDescriptor renderPassDescriptor];
    pass.colorAttachments[0].texture = t;
    pass.colorAttachments[0].loadAction = MTLLoadActionClear;
    pass.colorAttachments[0].storeAction = MTLStoreActionStore;
    pass.colorAttachments[0].clearColor = MTLClearColorMake(r, g, b, 1.0);
    id<MTLCommandBuffer> cb = [queue commandBuffer];
    id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:pass];
    enc.label = [NSString stringWithFormat:@"clear_%@", t.label];
    [enc endEncoding];
    [cb commit];
    [cb waitUntilCompleted];
    if (cb.error) fprintf(stderr, "cb error: %s\n", cb.error.localizedDescription.UTF8String);
}

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) { fprintf(stderr, "no device\n"); return 1; }
        printf("device: %s\n", device.name.UTF8String);

        id<MTLCommandQueue> queue = [device newCommandQueue];

        // Three identical-dims/format textures, distinct mip counts + colours.
        // Held in an array so ARC keeps them alive through the capture.
        NSMutableArray *keep = [NSMutableArray array];
        id<MTLTexture> red   = make(device, 1, "amb_red_mip1");
        id<MTLTexture> green = make(device, 3, "amb_green_mip3");
        id<MTLTexture> blue  = make(device, 7, "amb_blue_mip7");
        [keep addObject:red]; [keep addObject:green]; [keep addObject:blue];

        void (^work)(void) = ^{
            clear_to(queue, red,   1.0, 0.0, 0.0);  // red   BGRA 00 00 ff ff, mip 1
            clear_to(queue, green, 0.0, 1.0, 0.0);  // green BGRA 00 ff 00 ff, mip 3
            clear_to(queue, blue,  0.0, 0.0, 1.0);  // blue  BGRA ff 00 00 ff, mip 7
        };
        work();
        printf("phase 1: cleared 3x 64x64 BGRA: red/mip1 green/mip3 blue/mip7\n");

        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            printf("waiting for go-file %s\n", goFile); fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) { usleep(100000); if (++waited > 600) { fprintf(stderr, "no go-file\n"); return 1; } }
            work();
            printf("phase 2: re-ran inside capture\n");
        }
        printf("done (3 ambiguous-run textures held alive: %lu)\n", (unsigned long)keep.count);
    }
    return 0;
}
