// A fixture to investigate MIPMAP and ARRAY-SLICE texture fetch through the
// replayer (never exercised): a 2D-array texture with 2 slices and a full mip
// chain, slice 0 level 0 = red, slice 1 level 0 = green, mips generated. Blit
// to a second texture so the content is stored. The question: does
// GTReplayFetchTexture return level 0 only, all levels, and how are the two
// array slices represented (one record? per-slice records?).
//
// Two-phase (late boundary).
//
// Build:
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-mips \
//         fixtures/known-mips.m -framework Metal -framework Foundation
// Capture:
//   fixtures/capture-late.sh /tmp/known-mips captures/known-mips.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static void fill(id<MTLTexture> t, NSUInteger slice, NSUInteger w, NSUInteger h,
                 uint8_t b, uint8_t g, uint8_t r) {
    size_t bpr = w * 4;
    uint8_t *px = malloc(bpr * h);
    for (size_t i = 0; i < bpr * h; i += 4) { px[i]=b; px[i+1]=g; px[i+2]=r; px[i+3]=255; }
    [t replaceRegion:MTLRegionMake2D(0,0,w,h) mipmapLevel:0 slice:slice withBytes:px bytesPerRow:bpr bytesPerImage:bpr*h];
    free(px);
}

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) { fprintf(stderr, "no device\n"); return 1; }
        printf("device: %s\n", device.name.UTF8String);
        id<MTLCommandQueue> queue = [device newCommandQueue];

        const NSUInteger W = 64, H = 64, SLICES = 2;
        MTLTextureDescriptor *td = [[MTLTextureDescriptor alloc] init];
        td.textureType = MTLTextureType2DArray;
        td.pixelFormat = MTLPixelFormatBGRA8Unorm;
        td.width = W; td.height = H; td.arrayLength = SLICES;
        td.mipmapLevelCount = 7;  // 64 -> 1: levels 0..6
        td.usage = MTLTextureUsageShaderRead;
        td.storageMode = MTLStorageModeShared;
        id<MTLTexture> tex = [device newTextureWithDescriptor:td];
        tex.label = @"mips_array_src";
        td.usage = MTLTextureUsageShaderRead;
        id<MTLTexture> dst = [device newTextureWithDescriptor:td];
        dst.label = @"mips_array_dst";
        if (!tex || !dst) { fprintf(stderr, "alloc failed\n"); return 1; }

        fill(tex, 0, W, H, 0, 0, 255);   // slice 0 level 0 = red   (BGRA 00 00 ff ff)
        fill(tex, 1, W, H, 0, 255, 0);   // slice 1 level 0 = green (BGRA 00 ff 00 ff)
        printf("levels=%lu slices=%lu\n", (unsigned long)tex.mipmapLevelCount, (unsigned long)SLICES);

        void (^work)(void) = ^{
            id<MTLCommandBuffer> cb = [queue commandBuffer];
            id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
            [blit generateMipmapsForTexture:tex];               // fill lower levels
            [blit copyFromTexture:tex toTexture:dst];           // make tex a used blit source
            [blit endEncoding];
            [cb commit];
            [cb waitUntilCompleted];
            if (cb.error) fprintf(stderr, "cb error: %s\n", cb.error.localizedDescription.UTF8String);
        };

        work();
        printf("phase 1: generated mips + blit\n");
        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            printf("waiting for go-file %s\n", goFile); fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) { usleep(100000); if (++waited > 600) { fprintf(stderr, "no go-file\n"); return 1; } }
            work();
            printf("phase 2: re-ran inside capture\n");
        }
        printf("done (2D-array 64x64 x%lu slices, 7 mip levels, slice0=red slice1=green)\n",
               (unsigned long)SLICES);
    }
    return 0;
}
