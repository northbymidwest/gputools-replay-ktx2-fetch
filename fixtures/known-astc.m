// A fixture to investigate COMPRESSED (ASTC) texture fetch through the replayer
// (never exercised): a 64x64 ASTC_4x4_LDR texture filled with a known 16-byte
// block pattern, used via a blit so it is stored. The question: does
// GTReplayFetchTexture return the COMPRESSED blocks (4096 bytes = 256 blocks x
// 16 B, matching the pattern) or DECOMPRESSED pixels (16384 bytes RGBA), and
// what format does the reply report?
//
// The block bytes are a recognizable pattern (0x00..0x0F per block), not valid
// ASTC - this probes the transport layout, not decode correctness.
//
// Two-phase (late boundary).
//
// Build:
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-astc \
//         fixtures/known-astc.m -framework Metal -framework Foundation
// Capture:
//   fixtures/capture-late.sh /tmp/known-astc captures/known-astc.gputrace

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
        if (![device supportsFamily:MTLGPUFamilyApple1]) {
            fprintf(stderr, "known-astc: ASTC needs an Apple GPU family\n");
        }
        id<MTLCommandQueue> queue = [device newCommandQueue];

        const NSUInteger W = 64, H = 64;           // 16x16 = 256 ASTC 4x4 blocks
        const NSUInteger BLOCKS_X = W / 4, BLOCK_BYTES = 16;
        MTLTextureDescriptor *td = [MTLTextureDescriptor
            texture2DDescriptorWithPixelFormat:MTLPixelFormatASTC_4x4_LDR
                                         width:W height:H mipmapped:NO];
        td.usage = MTLTextureUsageShaderRead;
        td.storageMode = MTLStorageModeShared;
        id<MTLTexture> tex = [device newTextureWithDescriptor:td];
        tex.label = @"astc_src";
        td.usage = MTLTextureUsageShaderRead;
        id<MTLTexture> dst = [device newTextureWithDescriptor:td];
        dst.label = @"astc_dst";
        if (!tex || !dst) { fprintf(stderr, "known-astc: alloc failed (ASTC unsupported?)\n"); return 1; }

        // One recognizable 16-byte block pattern, repeated across all blocks.
        NSUInteger nblocks = BLOCKS_X * (H / 4);
        NSUInteger bytes = nblocks * BLOCK_BYTES;
        uint8_t *blocks = malloc(bytes);
        for (NSUInteger b = 0; b < nblocks; b++)
            for (int i = 0; i < 16; i++) blocks[b*16 + i] = (uint8_t)i;  // 0x00..0x0F
        // bytesPerRow for a compressed texture is per BLOCK ROW.
        [tex replaceRegion:MTLRegionMake2D(0,0,W,H) mipmapLevel:0
                 withBytes:blocks bytesPerRow:BLOCKS_X * BLOCK_BYTES];
        free(blocks);
        printf("filled %lu ASTC 4x4 blocks (%lu bytes), pattern 0x00..0x0F/block\n",
               (unsigned long)nblocks, (unsigned long)bytes);

        void (^work)(void) = ^{
            id<MTLCommandBuffer> cb = [queue commandBuffer];
            id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
            [blit copyFromTexture:tex toTexture:dst];   // make tex a used blit source
            [blit endEncoding];
            [cb commit];
            [cb waitUntilCompleted];
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
        printf("done (ASTC_4x4_LDR %lux%lu, compressed = 4096 B, decompressed = 16384 B)\n",
               (unsigned long)W, (unsigned long)H);
    }
    return 0;
}
