// A fixture to characterize COMBINED depth+stencil fetch through the replayer:
// renders a full-screen triangle at known depth (0.5) writing a known stencil
// reference (42) into a Depth32Float_Stencil8 attachment, so the fetched bytes
// reveal the combined format's actual per-pixel layout (depth offset/size,
// stencil offset/size, padding).
//
// Two-phase + blit-to-store, like known-depth.m.
//
// Build:
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-depth-stencil \
//         fixtures/known-depth-stencil.m -framework Metal -framework Foundation
// Capture:
//   fixtures/capture-late.sh /tmp/known-depth-stencil captures/known-depth-stencil.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdio.h>
#include <unistd.h>

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
        if (!device) { fprintf(stderr, "no device\n"); return 1; }
        printf("device: %s\n", device.name.UTF8String);
        id<MTLCommandQueue> queue = [device newCommandQueue];

        const NSUInteger W = 64, H = 64;
        const MTLPixelFormat DSFMT = MTLPixelFormatDepth32Float_Stencil8;

        MTLTextureDescriptor *cd = [MTLTextureDescriptor
            texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm width:W height:H mipmapped:NO];
        cd.usage = MTLTextureUsageRenderTarget; cd.storageMode = MTLStorageModePrivate;
        id<MTLTexture> color = [device newTextureWithDescriptor:cd];

        MTLTextureDescriptor *dd = [MTLTextureDescriptor
            texture2DDescriptorWithPixelFormat:DSFMT width:W height:H mipmapped:NO];
        dd.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
        dd.storageMode = MTLStorageModePrivate;
        id<MTLTexture> ds_src = [device newTextureWithDescriptor:dd];   // rendered into, blit source
        dd.usage = MTLTextureUsageRenderTarget;
        id<MTLTexture> ds_dst = [device newTextureWithDescriptor:dd];   // blit dest
        ds_src.label = @"ds_src"; ds_dst.label = @"ds_dst";
        if (!color || !ds_src || !ds_dst) { fprintf(stderr, "alloc failed\n"); return 1; }

        NSError *err = nil;
        id<MTLLibrary> lib = [device newLibraryWithSource:kSource options:nil error:&err];
        if (!lib) { fprintf(stderr, "compile: %s\n", err.localizedDescription.UTF8String); return 1; }
        MTLRenderPipelineDescriptor *pd = [[MTLRenderPipelineDescriptor alloc] init];
        pd.vertexFunction = [lib newFunctionWithName:@"v_main"];
        pd.fragmentFunction = [lib newFunctionWithName:@"f_main"];
        pd.colorAttachments[0].pixelFormat = MTLPixelFormatBGRA8Unorm;
        pd.depthAttachmentPixelFormat = DSFMT;
        pd.stencilAttachmentPixelFormat = DSFMT;
        id<MTLRenderPipelineState> pso = [device newRenderPipelineStateWithDescriptor:pd error:&err];
        if (!pso) { fprintf(stderr, "pipeline: %s\n", err.localizedDescription.UTF8String); return 1; }

        // Depth: always write 0.5. Stencil: always pass, REPLACE with the ref (42).
        MTLDepthStencilDescriptor *dsd = [[MTLDepthStencilDescriptor alloc] init];
        dsd.depthCompareFunction = MTLCompareFunctionAlways;
        dsd.depthWriteEnabled = YES;
        MTLStencilDescriptor *sc = [[MTLStencilDescriptor alloc] init];
        sc.stencilCompareFunction = MTLCompareFunctionAlways;
        sc.depthStencilPassOperation = MTLStencilOperationReplace;
        sc.writeMask = 0xFF;
        dsd.frontFaceStencil = sc;
        dsd.backFaceStencil = sc;
        id<MTLDepthStencilState> dss = [device newDepthStencilStateWithDescriptor:dsd];

        void (^render)(void) = ^{
            MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
            rp.colorAttachments[0].texture = color;
            rp.colorAttachments[0].loadAction = MTLLoadActionClear;
            rp.colorAttachments[0].clearColor = MTLClearColorMake(0, 0, 0, 1);
            rp.colorAttachments[0].storeAction = MTLStoreActionStore;
            rp.depthAttachment.texture = ds_src;
            rp.depthAttachment.loadAction = MTLLoadActionClear;
            rp.depthAttachment.clearDepth = 1.0;
            rp.depthAttachment.storeAction = MTLStoreActionStore;
            rp.stencilAttachment.texture = ds_src;
            rp.stencilAttachment.loadAction = MTLLoadActionClear;
            rp.stencilAttachment.clearStencil = 0;
            rp.stencilAttachment.storeAction = MTLStoreActionStore;
            id<MTLCommandBuffer> cb = [queue commandBuffer];
            id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
            [enc setRenderPipelineState:pso];
            [enc setDepthStencilState:dss];
            [enc setStencilReferenceValue:42];
            [enc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
            [enc endEncoding];
            // Store the rendered content by making ds_src a blit source. Depth
            // and stencil aspects are copied separately (a combined-format blit
            // requires per-aspect copies).
            id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
            [blit copyFromTexture:ds_src sourceSlice:0 sourceLevel:0
                     sourceOrigin:MTLOriginMake(0,0,0) sourceSize:MTLSizeMake(W,H,1)
                        toTexture:ds_dst destinationSlice:0 destinationLevel:0
                destinationOrigin:MTLOriginMake(0,0,0)];
            [blit endEncoding];
            [cb commit];
            [cb waitUntilCompleted];
            if (cb.error) fprintf(stderr, "cb error: %s\n", cb.error.localizedDescription.UTF8String);
        };

        render();
        printf("phase 1: depth 0.5, stencil 42\n");
        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            printf("waiting for go-file %s\n", goFile); fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) { usleep(100000); if (++waited > 600) { fprintf(stderr, "no go-file\n"); return 1; } }
            render();
            printf("phase 2: re-rendered inside capture\n");
        }
        printf("done (Depth32Float_Stencil8 %lux%lu)\n", (unsigned long)W, (unsigned long)H);
    }
    return 0;
}
