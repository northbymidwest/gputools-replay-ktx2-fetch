// A fixture app to characterize DEPTH texture fetch through the replayer (never
// done in the campaign): renders a full-screen triangle at a known constant
// depth into a Depth32Float attachment, so the fetched depth buffer has an
// exact ground-truth value (0.5) everywhere.
//
// Two-phase (late boundary), like known-buffers.m, so the depth texture
// pre-exists the capture boundary and is written by a captured command.
//
// Build:
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-depth \
//         fixtures/known-depth.m -framework Metal -framework Foundation
// Capture:
//   fixtures/capture-late.sh /tmp/known-depth captures/known-depth.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdio.h>
#include <unistd.h>

// Metal NDC z is [0,1]; a vertex at z=0.5,w=1 writes depth 0.5. A full-screen
// triangle (three oversized verts) covers every pixel, so the whole depth
// buffer becomes 0.5 where the clear (1.0) is overwritten.
static NSString *const kSource =
    @"#include <metal_stdlib>\n"
    @"using namespace metal;\n"
    @"vertex float4 v_main(uint vid [[vertex_id]]) {\n"
    @"    float2 p[3] = { float2(-1,-3), float2(-1,1), float2(3,1) };\n"
    @"    return float4(p[vid], 0.5, 1.0);\n"
    @"}\n"
    @"fragment float4 f_main() { return float4(1,1,1,1); }\n";

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) { fprintf(stderr, "known-depth: no device\n"); return 1; }
        printf("device: %s\n", device.name.UTF8String);
        id<MTLCommandQueue> queue = [device newCommandQueue];

        const NSUInteger W = 64, H = 64;
        MTLTextureDescriptor *cd = [MTLTextureDescriptor
            texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                         width:W height:H mipmapped:NO];
        cd.usage = MTLTextureUsageRenderTarget;
        cd.storageMode = MTLStorageModePrivate;
        id<MTLTexture> color = [device newTextureWithDescriptor:cd];
        color.label = @"depth_fixture_color";

        MTLTextureDescriptor *dd = [MTLTextureDescriptor
            texture2DDescriptorWithPixelFormat:MTLPixelFormatDepth32Float
                                         width:W height:H mipmapped:NO];
        dd.usage = MTLTextureUsageRenderTarget;
        dd.storageMode = MTLStorageModePrivate;
        id<MTLTexture> depth = [device newTextureWithDescriptor:dd];
        depth.label = @"depth_fixture_depth";
        // A second depth texture to blit into: making `depth` a blit SOURCE is
        // what gets its rendered content snapshotted for fetch (a write-only
        // render target is not stored - measured, same as known-textures'
        // clear-only rows). `depth` also needs ShaderRead usage to be a source.
        dd.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
        id<MTLTexture> depth_src = [device newTextureWithDescriptor:dd];
        depth_src.label = @"depth_fixture_depth_src";
        if (!color || !depth || !depth_src) { fprintf(stderr, "known-depth: texture alloc failed\n"); return 1; }

        NSError *err = nil;
        id<MTLLibrary> lib = [device newLibraryWithSource:kSource options:nil error:&err];
        if (!lib) { fprintf(stderr, "known-depth: compile: %s\n", err.localizedDescription.UTF8String); return 1; }
        MTLRenderPipelineDescriptor *pd = [[MTLRenderPipelineDescriptor alloc] init];
        pd.vertexFunction = [lib newFunctionWithName:@"v_main"];
        pd.fragmentFunction = [lib newFunctionWithName:@"f_main"];
        pd.colorAttachments[0].pixelFormat = MTLPixelFormatBGRA8Unorm;
        pd.depthAttachmentPixelFormat = MTLPixelFormatDepth32Float;
        id<MTLRenderPipelineState> pso = [device newRenderPipelineStateWithDescriptor:pd error:&err];
        if (!pso) { fprintf(stderr, "known-depth: pipeline: %s\n", err.localizedDescription.UTF8String); return 1; }

        MTLDepthStencilDescriptor *dsd = [[MTLDepthStencilDescriptor alloc] init];
        dsd.depthCompareFunction = MTLCompareFunctionAlways;  // always write
        dsd.depthWriteEnabled = YES;
        id<MTLDepthStencilState> dss = [device newDepthStencilStateWithDescriptor:dsd];

        void (^render)(void) = ^{
            MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
            rp.colorAttachments[0].texture = color;
            rp.colorAttachments[0].loadAction = MTLLoadActionClear;
            rp.colorAttachments[0].clearColor = MTLClearColorMake(0, 0, 0, 1);
            rp.colorAttachments[0].storeAction = MTLStoreActionStore;
            rp.depthAttachment.texture = depth_src;
            rp.depthAttachment.loadAction = MTLLoadActionClear;
            rp.depthAttachment.clearDepth = 1.0;
            rp.depthAttachment.storeAction = MTLStoreActionStore;
            id<MTLCommandBuffer> cb = [queue commandBuffer];
            id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
            [enc setRenderPipelineState:pso];
            [enc setDepthStencilState:dss];
            [enc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
            [enc endEncoding];
            // Blit depth_src -> depth, making depth_src a used blit source so
            // its rendered 0.5 content is snapshotted for fetch (both endpoints
            // of a blit get stored, per known-textures).
            id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
            [blit copyFromTexture:depth_src
                      sourceSlice:0 sourceLevel:0
                     sourceOrigin:MTLOriginMake(0, 0, 0)
                       sourceSize:MTLSizeMake(W, H, 1)
                        toTexture:depth
                 destinationSlice:0 destinationLevel:0
                destinationOrigin:MTLOriginMake(0, 0, 0)];
            [blit endEncoding];
            [cb commit];
            [cb waitUntilCompleted];
        };

        render();  // phase 1
        printf("phase 1: rendered full-screen triangle at depth 0.5\n");

        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            printf("phase 1 done; waiting for go-file %s\n", goFile);
            fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) {
                usleep(100000);
                if (++waited > 600) { fprintf(stderr, "known-depth: go-file never appeared\n"); return 1; }
            }
            render();  // phase 2, inside the capture
            printf("phase 2: re-rendered inside the capture\n");
        }
        printf("done (depth32float %lux%lu, expect 0.5 everywhere)\n",
               (unsigned long)W, (unsigned long)H);
    }
    return 0;
}
