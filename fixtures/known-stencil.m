// A fixture to investigate STENCIL fetch through the replayer:
//   (A) a base Stencil8 texture (stencil = 42), and
//   (B) the stencil aspect of a combined Depth32Float_Stencil8 texture, exposed
//       as an X32_Stencil8 texture view (stencil = 77).
// Both are used (rendered + blit / sampled) so they are stored, two-phase.
//
// The question: is stencil returned by GTReplayFetchTexture at all, and in what
// format/size - given the earlier finding that a combined depth/stencil texture
// fetches only its depth aspect.
//
// Build:
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-stencil \
//         fixtures/known-stencil.m -framework Metal -framework Foundation
// Capture:
//   fixtures/capture-late.sh /tmp/known-stencil captures/known-stencil.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdio.h>
#include <unistd.h>

static NSString *const kSrc =
    @"#include <metal_stdlib>\n"
    @"using namespace metal;\n"
    @"vertex float4 v_main(uint vid [[vertex_id]]) {\n"
    @"    float2 p[3] = { float2(-1,-3), float2(-1,1), float2(3,1) };\n"
    @"    return float4(p[vid], 0.5, 1.0);\n"
    @"}\n"
    @"fragment float4 f_main() { return float4(1,1,1,1); }\n"
    // Samples an X32_Stencil8 view (stencil is uint) so the view is a used
    // resource; writes it to a buffer.
    @"kernel void read_stencil(texture2d<uint> s [[texture(0)]],\n"
    @"                         device uint* out [[buffer(0)]],\n"
    @"                         uint2 gid [[thread_position_in_grid]]) {\n"
    @"    out[gid.y*64+gid.x] = s.read(gid).r;\n"
    @"}\n";

static id<MTLRenderPipelineState> makePSO(id<MTLDevice> dev, id<MTLLibrary> lib,
        MTLPixelFormat depthFmt, MTLPixelFormat stencilFmt) {
    MTLRenderPipelineDescriptor *pd = [[MTLRenderPipelineDescriptor alloc] init];
    pd.vertexFunction = [lib newFunctionWithName:@"v_main"];
    pd.fragmentFunction = [lib newFunctionWithName:@"f_main"];
    pd.colorAttachments[0].pixelFormat = MTLPixelFormatBGRA8Unorm;
    pd.depthAttachmentPixelFormat = depthFmt;
    pd.stencilAttachmentPixelFormat = stencilFmt;
    NSError *e = nil;
    id<MTLRenderPipelineState> p = [dev newRenderPipelineStateWithDescriptor:pd error:&e];
    if (!p) fprintf(stderr, "pso: %s\n", e.localizedDescription.UTF8String);
    return p;
}

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) { fprintf(stderr, "no device\n"); return 1; }
        printf("device: %s\n", device.name.UTF8String);
        id<MTLCommandQueue> queue = [device newCommandQueue];
        const NSUInteger W = 64, H = 64;
        NSError *err = nil;
        id<MTLLibrary> lib = [device newLibraryWithSource:kSrc options:nil error:&err];
        if (!lib) { fprintf(stderr, "compile: %s\n", err.localizedDescription.UTF8String); return 1; }

        MTLTextureDescriptor *cd = [MTLTextureDescriptor
            texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm width:W height:H mipmapped:NO];
        cd.usage = MTLTextureUsageRenderTarget; cd.storageMode = MTLStorageModePrivate;
        id<MTLTexture> color = [device newTextureWithDescriptor:cd];

        // (A) base Stencil8, and its blit destination.
        MTLTextureDescriptor *sd = [MTLTextureDescriptor
            texture2DDescriptorWithPixelFormat:MTLPixelFormatStencil8 width:W height:H mipmapped:NO];
        sd.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
        sd.storageMode = MTLStorageModePrivate;
        id<MTLTexture> s8 = [device newTextureWithDescriptor:sd]; s8.label = @"stencil8_src";
        id<MTLTexture> s8dst = [device newTextureWithDescriptor:sd]; s8dst.label = @"stencil8_dst";

        // (B) combined Depth32Float_Stencil8 with pixel-format-view usage.
        MTLTextureDescriptor *dsd = [MTLTextureDescriptor
            texture2DDescriptorWithPixelFormat:MTLPixelFormatDepth32Float_Stencil8 width:W height:H mipmapped:NO];
        dsd.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead | MTLTextureUsagePixelFormatView;
        dsd.storageMode = MTLStorageModePrivate;
        id<MTLTexture> ds = [device newTextureWithDescriptor:dsd]; ds.label = @"combined_ds";
        id<MTLTexture> dsDst = [device newTextureWithDescriptor:dsd]; dsDst.label = @"combined_ds_dst";
        id<MTLTexture> stencilView = [ds newTextureViewWithPixelFormat:MTLPixelFormatX32_Stencil8];
        stencilView.label = @"combined_stencil_view";
        if (!color || !s8 || !s8dst || !ds || !stencilView) { fprintf(stderr, "alloc failed\n"); return 1; }

        id<MTLRenderPipelineState> psoS = makePSO(device, lib, MTLPixelFormatInvalid, MTLPixelFormatStencil8);
        id<MTLRenderPipelineState> psoDS = makePSO(device, lib, MTLPixelFormatDepth32Float_Stencil8, MTLPixelFormatDepth32Float_Stencil8);
        if (!psoS || !psoDS) return 1;

        MTLStencilDescriptor *sc = [[MTLStencilDescriptor alloc] init];
        sc.stencilCompareFunction = MTLCompareFunctionAlways;
        sc.depthStencilPassOperation = MTLStencilOperationReplace; sc.writeMask = 0xFF;
        MTLDepthStencilDescriptor *dss1 = [[MTLDepthStencilDescriptor alloc] init];
        dss1.frontFaceStencil = sc; dss1.backFaceStencil = sc;
        id<MTLDepthStencilState> stateS = [device newDepthStencilStateWithDescriptor:dss1];
        MTLDepthStencilDescriptor *dss2 = [[MTLDepthStencilDescriptor alloc] init];
        dss2.depthCompareFunction = MTLCompareFunctionAlways; dss2.depthWriteEnabled = YES;
        dss2.frontFaceStencil = sc; dss2.backFaceStencil = sc;
        id<MTLDepthStencilState> stateDS = [device newDepthStencilStateWithDescriptor:dss2];

        id<MTLBuffer> out = [device newBufferWithLength:W*H*sizeof(uint32_t) options:MTLResourceStorageModeShared];
        out.label = @"stencil_readout";
        id<MTLComputePipelineState> cpso = [device newComputePipelineStateWithFunction:
            [lib newFunctionWithName:@"read_stencil"] error:&err];

        void (^work)(void) = ^{
            id<MTLCommandBuffer> cb = [queue commandBuffer];
            // (A) render stencil 42 into the base Stencil8, then blit to store it.
            MTLRenderPassDescriptor *rpA = [MTLRenderPassDescriptor renderPassDescriptor];
            rpA.colorAttachments[0].texture = color;
            rpA.colorAttachments[0].loadAction = MTLLoadActionClear;
            rpA.colorAttachments[0].storeAction = MTLStoreActionStore;
            rpA.stencilAttachment.texture = s8;
            rpA.stencilAttachment.loadAction = MTLLoadActionClear;
            rpA.stencilAttachment.clearStencil = 0;
            rpA.stencilAttachment.storeAction = MTLStoreActionStore;
            id<MTLRenderCommandEncoder> eA = [cb renderCommandEncoderWithDescriptor:rpA];
            [eA setRenderPipelineState:psoS]; [eA setDepthStencilState:stateS];
            [eA setStencilReferenceValue:42];
            [eA drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
            [eA endEncoding];
            id<MTLBlitCommandEncoder> bl = [cb blitCommandEncoder];
            [bl copyFromTexture:s8 toTexture:s8dst];
            [bl endEncoding];
            // (B) render depth 0.5 + stencil 77 into the combined texture.
            MTLRenderPassDescriptor *rpB = [MTLRenderPassDescriptor renderPassDescriptor];
            rpB.colorAttachments[0].texture = color;
            rpB.colorAttachments[0].loadAction = MTLLoadActionClear;
            rpB.colorAttachments[0].storeAction = MTLStoreActionStore;
            rpB.depthAttachment.texture = ds; rpB.depthAttachment.loadAction = MTLLoadActionClear;
            rpB.depthAttachment.clearDepth = 1.0; rpB.depthAttachment.storeAction = MTLStoreActionStore;
            rpB.stencilAttachment.texture = ds; rpB.stencilAttachment.loadAction = MTLLoadActionClear;
            rpB.stencilAttachment.clearStencil = 0; rpB.stencilAttachment.storeAction = MTLStoreActionStore;
            id<MTLRenderCommandEncoder> eB = [cb renderCommandEncoderWithDescriptor:rpB];
            [eB setRenderPipelineState:psoDS]; [eB setDepthStencilState:stateDS];
            [eB setStencilReferenceValue:77];
            [eB drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
            [eB endEncoding];
            // Blit the combined texture so its depth+stencil content is stored
            // (ds becomes a used blit source).
            id<MTLBlitCommandEncoder> bl2 = [cb blitCommandEncoder];
            [bl2 copyFromTexture:ds toTexture:dsDst];
            [bl2 endEncoding];
            // Sample the X32_Stencil8 view so it is a used resource.
            id<MTLComputeCommandEncoder> ce = [cb computeCommandEncoder];
            [ce setComputePipelineState:cpso];
            [ce setTexture:stencilView atIndex:0];
            [ce setBuffer:out offset:0 atIndex:0];
            [ce dispatchThreads:MTLSizeMake(W,H,1) threadsPerThreadgroup:MTLSizeMake(8,8,1)];
            [ce endEncoding];
            [cb commit]; [cb waitUntilCompleted];
            if (cb.error) fprintf(stderr, "cb error: %s\n", cb.error.localizedDescription.UTF8String);
        };

        work();
        printf("phase 1: base stencil=42, combined stencil=77, readout[10]=%u\n", ((uint32_t*)out.contents)[10]);
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
