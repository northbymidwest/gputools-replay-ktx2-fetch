// A fixture to investigate 3D (volume) texture fetch through the replayer:
// a 16x16x4 BGRA8Unorm 3D texture, each z-slice a distinct colour, blit-stored.
// The question: does GTReplayFetchTexture return the whole volume (depth
// reported 4, size 16*16*4*4) or a single z-slice (depth 1)?
//
// Two-phase (late boundary).
//
// Build:
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-3d \
//         fixtures/known-3d.m -framework Metal -framework Foundation
// Capture:
//   fixtures/capture-late.sh /tmp/known-3d captures/known-3d.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) { fprintf(stderr, "no device\n"); return 1; }
        printf("device: %s\n", device.name.UTF8String);
        id<MTLCommandQueue> queue = [device newCommandQueue];

        const NSUInteger W = 16, H = 16, D = 4;
        MTLTextureDescriptor *td = [[MTLTextureDescriptor alloc] init];
        td.textureType = MTLTextureType3D;
        td.pixelFormat = MTLPixelFormatBGRA8Unorm;
        td.width = W; td.height = H; td.depth = D;
        td.usage = MTLTextureUsageShaderRead;
        td.storageMode = MTLStorageModeShared;
        id<MTLTexture> tex = [device newTextureWithDescriptor:td]; tex.label = @"vol_src";
        id<MTLTexture> dst = [device newTextureWithDescriptor:td]; dst.label = @"vol_dst";
        if (!tex || !dst) { fprintf(stderr, "alloc failed\n"); return 1; }

        // Each z-slice a distinct blue value (10, 20, 30, 40), so a fetched
        // volume would show 4 distinct values and a single slice just one.
        size_t bpr = W * 4, bpi = bpr * H;
        uint8_t *px = malloc(bpi);
        for (NSUInteger z = 0; z < D; z++) {
            uint8_t b = (uint8_t)((z + 1) * 10);
            for (size_t i = 0; i < bpi; i += 4) { px[i]=b; px[i+1]=0; px[i+2]=0; px[i+3]=255; }
            [tex replaceRegion:MTLRegionMake3D(0,0,z,W,H,1) mipmapLevel:0 slice:0
                     withBytes:px bytesPerRow:bpr bytesPerImage:bpi];
        }
        free(px);
        printf("3D %lux%lux%lu, z-slices blue=10,20,30,40\n",
               (unsigned long)W,(unsigned long)H,(unsigned long)D);

        void (^work)(void) = ^{
            id<MTLCommandBuffer> cb = [queue commandBuffer];
            id<MTLBlitCommandEncoder> bl = [cb blitCommandEncoder];
            [bl copyFromTexture:tex toTexture:dst];
            [bl endEncoding];
            [cb commit]; [cb waitUntilCompleted];
            if (cb.error) fprintf(stderr, "cb error: %s\n", cb.error.localizedDescription.UTF8String);
        };
        work();
        printf("phase 1: blit\n");
        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            printf("waiting for go-file %s\n", goFile); fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) { usleep(100000); if (++waited > 600) { fprintf(stderr, "no go-file\n"); return 1; } }
            work();
            printf("phase 2: re-ran inside capture\n");
        }
        printf("done\n");
    }
    return 0;
}
