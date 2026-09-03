// A fixture to investigate PLANAR (biplanar YCbCr) texture fetch through the
// replayer, and the `plane` fetch parameter. Planar textures in Metal are
// IOSurface/CVPixelBuffer-backed; this creates a 64x64 4:2:0 biplanar buffer
// with known luma (Y=128) and chroma (Cb=100, Cr=150), wraps each plane as an
// MTLTexture, and samples both in a compute pass so they are used resources.
//
// Two-phase (late boundary). The open question: does the replayer capture an
// IOSurface-backed planar texture, and does GTReplayFetchTexture with
// setPlane:0/1 return the Y and CbCr planes?
//
// Build (note the extra CoreVideo framework):
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-ycbcr \
//         fixtures/known-ycbcr.m -framework Metal -framework Foundation \
//         -framework CoreVideo
// Capture:
//   fixtures/capture-late.sh /tmp/known-ycbcr captures/known-ycbcr.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <CoreVideo/CoreVideo.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static NSString *const kSource =
    @"#include <metal_stdlib>\n"
    @"using namespace metal;\n"
    @"kernel void sample_yuv(texture2d<float> y [[texture(0)]],\n"
    @"                       texture2d<float> cbcr [[texture(1)]],\n"
    @"                       device float* out [[buffer(0)]],\n"
    @"                       uint2 gid [[thread_position_in_grid]]) {\n"
    @"    constexpr sampler s(coord::pixel);\n"
    @"    float Y = y.sample(s, float2(gid)).r;\n"
    @"    float2 C = cbcr.sample(s, float2(gid) * 0.5).rg;\n"
    @"    out[gid.y * 64 + gid.x] = Y + C.x + C.y;\n"
    @"}\n";

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) { fprintf(stderr, "no device\n"); return 1; }
        printf("device: %s\n", device.name.UTF8String);
        id<MTLCommandQueue> queue = [device newCommandQueue];

        // A biplanar 4:2:0 pixel buffer, Metal-compatible.
        CVPixelBufferRef pb = NULL;
        NSDictionary *attrs = @{ (id)kCVPixelBufferMetalCompatibilityKey: @YES };
        CVReturn r = CVPixelBufferCreate(kCFAllocatorDefault, 64, 64,
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
            (__bridge CFDictionaryRef)attrs, &pb);
        if (r != kCVReturnSuccess || !pb) { fprintf(stderr, "CVPixelBufferCreate failed: %d\n", r); return 1; }

        CVPixelBufferLockBaseAddress(pb, 0);
        uint8_t *y = CVPixelBufferGetBaseAddressOfPlane(pb, 0);
        size_t yStride = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
        for (size_t row = 0; row < 64; row++) memset(y + row * yStride, 128, 64);  // Y = 128
        uint8_t *cc = CVPixelBufferGetBaseAddressOfPlane(pb, 1);
        size_t ccStride = CVPixelBufferGetBytesPerRowOfPlane(pb, 1);
        for (size_t row = 0; row < 32; row++)
            for (size_t col = 0; col < 32; col++) { cc[row*ccStride + col*2] = 100; cc[row*ccStride + col*2 + 1] = 150; }
        CVPixelBufferUnlockBaseAddress(pb, 0);
        printf("planes: Y stride=%zu (64x64=128), CbCr stride=%zu (32x32, Cb=100 Cr=150)\n",
               yStride, ccStride);

        // Wrap each plane as an MTLTexture via a texture cache.
        CVMetalTextureCacheRef cache = NULL;
        if (CVMetalTextureCacheCreate(NULL, NULL, device, NULL, &cache) != kCVReturnSuccess) {
            fprintf(stderr, "texture cache create failed\n"); return 1;
        }
        CVMetalTextureRef yRef = NULL, cRef = NULL;
        CVMetalTextureCacheCreateTextureFromImage(NULL, cache, pb, NULL,
            MTLPixelFormatR8Unorm, 64, 64, 0, &yRef);
        CVMetalTextureCacheCreateTextureFromImage(NULL, cache, pb, NULL,
            MTLPixelFormatRG8Unorm, 32, 32, 1, &cRef);
        if (!yRef || !cRef) { fprintf(stderr, "plane texture create failed\n"); return 1; }
        id<MTLTexture> yTex = CVMetalTextureGetTexture(yRef);
        id<MTLTexture> cTex = CVMetalTextureGetTexture(cRef);
        yTex.label = @"ycbcr_luma"; cTex.label = @"ycbcr_chroma";

        id<MTLBuffer> out = [device newBufferWithLength:64*64*sizeof(float)
                                                options:MTLResourceStorageModeShared];
        out.label = @"ycbcr_out";

        NSError *err = nil;
        id<MTLLibrary> lib = [device newLibraryWithSource:kSource options:nil error:&err];
        if (!lib) { fprintf(stderr, "compile: %s\n", err.localizedDescription.UTF8String); return 1; }
        id<MTLComputePipelineState> pso = [device newComputePipelineStateWithFunction:
            [lib newFunctionWithName:@"sample_yuv"] error:&err];
        if (!pso) { fprintf(stderr, "pipeline: %s\n", err.localizedDescription.UTF8String); return 1; }

        void (^run)(void) = ^{
            id<MTLCommandBuffer> cb = [queue commandBuffer];
            id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
            [enc setComputePipelineState:pso];
            [enc setTexture:yTex atIndex:0];
            [enc setTexture:cTex atIndex:1];
            [enc setBuffer:out offset:0 atIndex:0];
            [enc dispatchThreads:MTLSizeMake(64,64,1) threadsPerThreadgroup:MTLSizeMake(8,8,1)];
            [enc endEncoding];
            [cb commit];
            [cb waitUntilCompleted];
            if (cb.error) fprintf(stderr, "cb error: %s\n", cb.error.localizedDescription.UTF8String);
        };

        run();
        printf("phase 1: sampled both planes\n");
        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            printf("waiting for go-file %s\n", goFile); fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) { usleep(100000); if (++waited > 600) { fprintf(stderr, "no go-file\n"); return 1; } }
            run();
            printf("phase 2: re-sampled inside capture\n");
        }
        printf("done (biplanar 420 YpCbCr, Y=128 Cb=100 Cr=150)\n");
        // Keep refs alive to end of capture.
        (void)yRef; (void)cRef; (void)cache; (void)pb;
    }
    return 0;
}
