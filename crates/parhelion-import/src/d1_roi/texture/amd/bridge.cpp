// AMD computes padding and GPU addresses. Rust owns resource ordering, bounds and buffers.
#include "bridge.h"
#include "addrinterface.h"
#include "amdgpu_asic_addr.h"
#include <cstdlib>
#include <cstring>
#include <limits>

namespace {
void* ADDR_API allocate(const ADDR_ALLOCSYSMEM_INPUT* request) {
    return std::malloc(request->sizeInBytes);
}
ADDR_E_RETURNCODE ADDR_API deallocate(const ADDR_FREESYSMEM_INPUT* request) {
    std::free(request->pVirtAddr);
    return ADDR_OK;
}

// Xbox One color-surface parameters, independent of the machine running the importer.
// Each row describes 8 banks, 4 pipes, and a 256-byte interleave. Bits are per element,
// so compressed textures use their 64-bit or 128-bit block storage class.
bool color_tiles(uint32_t bits, ADDR_TILEINFO& tiles) {
    tiles.banks = 8;
    tiles.bankWidth = 1;
    tiles.pipeConfig = ADDR_PIPECFG_P4_16x16;
    switch (bits) {
    case 8:   tiles.bankHeight = 4; tiles.macroAspectRatio = 4; tiles.tileSplitBytes = 256; break;
    case 16:  tiles.bankHeight = 2; tiles.macroAspectRatio = 2; tiles.tileSplitBytes = 256; break;
    case 32:  tiles.bankHeight = 1; tiles.macroAspectRatio = 2; tiles.tileSplitBytes = 512; break;
    case 64:  tiles.bankHeight = 1; tiles.macroAspectRatio = 2; tiles.tileSplitBytes = 1024; break;
    case 128: tiles.bankHeight = 1; tiles.macroAspectRatio = 2; tiles.tileSplitBytes = 2048; break;
    default: return false;
    }
    return true;
}
}

extern "C" void* parhelion_addr_create() {
    ADDR_CREATE_INPUT request = {};
    ADDR_CREATE_OUTPUT result = {};
    request.size = sizeof(request);
    result.size = sizeof(result);
    request.chipEngine = 0xA; // CIASICIDGFXENGINE_SOUTHERNISLAND in AMD's addrlib.h.
    request.chipFamily = FAMILY_SI;
    request.chipRevision = 5;
    request.callbacks.allocSysMem = allocate;
    request.callbacks.freeSysMem = deallocate;
    request.createFlags.fillSizeFields = 1;
    request.createFlags.noCubeMipSlicesPad = 1;
    // GB_ADDR_CONFIG: 256-byte interleave and 2-KiB row. The library requires a tile
    // table during construction even with useTileIndex disabled. Surface calls supply
    // their complete tile parameters, never a host GPU table or a table index.
    request.regValue.gbAddrConfig = 0x10000002;
    request.regValue.noOfBanks = 1;
    uint32_t bootstrap[32];
    for (auto& entry : bootstrap) entry = 0x149;
    bootstrap[8] = 0x144;
    request.regValue.pTileConfig = bootstrap;
    request.regValue.noOfEntries = 32;
    return AddrCreate(&request, &result) == ADDR_OK ? result.hLib : nullptr;
}

extern "C" void parhelion_addr_destroy(void* library) {
    if (library) AddrDestroy(library);
}

extern "C" int32_t parhelion_addr_surface(
    void* library, const SurfaceRequest* request, Surface* result) {
    if (!library || !request || !result) return -1;
    ADDR_TILEINFO tiles = {};
    if (!color_tiles(request->bits, tiles)) return -1;
    ADDR_COMPUTE_SURFACE_INFO_INPUT input = {};
    ADDR_COMPUTE_SURFACE_INFO_OUTPUT output = {};
    input.size = sizeof(input);
    output.size = sizeof(output);
    switch (request->mode) {
    case 13: input.tileMode = ADDR_TM_1D_TILED_THIN1; break;
    case 14: input.tileMode = ADDR_TM_2D_TILED_THIN1; break;
    default: return -1;
    }
    input.tileType = ADDR_NON_DISPLAYABLE;
    input.tileIndex = -1;
    input.bpp = request->bits;
    // Format is intentionally unspecified: width, height and basePitch are all elements.
    // Rust has already changed BC pixels into blocks, so AMD must not do it a second time.
    input.width = request->width;
    input.height = request->height;
    input.numSlices = request->slices;
    input.numSamples = 1;
    input.mipLevel = request->mip;
    input.numMipLevels = request->mips;
    input.basePitch = request->base_pitch;
    input.flags.texture = 1;
    input.flags.pow2Pad = request->mips > 1;
    input.flags.volume = request->volume != 0;
    input.pTileInfo = &tiles;
    ADDR_TILEINFO resolved = {};
    output.pTileInfo = &resolved;
    const auto status = AddrComputeSurfaceInfo(library, &input, &output);
    if (status != ADDR_OK) return static_cast<int32_t>(status);
    result->bytes = output.surfSize;
    result->pitch = output.pitch;
    result->height = output.height;
    result->slices = output.depth;
    result->mode = output.tileMode;
    result->type = output.tileType;
    result->alignment = output.baseAlign;
    result->bits = output.bpp;
    result->banks = resolved.banks;
    result->bank_width = resolved.bankWidth;
    result->bank_height = resolved.bankHeight;
    result->aspect = resolved.macroAspectRatio;
    result->split = resolved.tileSplitBytes;
    result->pipe = resolved.pipeConfig;
    return 0;
}

extern "C" int32_t parhelion_addr_copy(void* library, const Surface* surface,
    uint32_t width, uint32_t height, uint32_t first_slice, uint32_t slices,
    const uint8_t* source, size_t source_size, uint8_t* target, size_t target_size) {
    if (!library || !surface || !source || !target || !width || !height || !slices) return -1;
    if (width > surface->pitch || height > surface->height ||
        first_slice > surface->slices || slices > surface->slices - first_slice) return -1;
    if (source_size != surface->bytes || surface->bits < 8 || surface->bits > 128 ||
        (surface->bits & (surface->bits - 1)) != 0) return -1;
    const uint32_t bytes = surface->bits / 8;
    const uint64_t required = uint64_t(width) * height * slices * bytes;
    if (required != target_size || required > std::numeric_limits<size_t>::max()) return -1;
    ADDR_TILEINFO tiles = {surface->banks, surface->bank_width, surface->bank_height,
        surface->aspect, surface->split, static_cast<AddrPipeCfg>(surface->pipe)};
    ADDR_COMPUTE_SURFACE_ADDRFROMCOORD_INPUT input = {};
    ADDR_COMPUTE_SURFACE_ADDRFROMCOORD_OUTPUT output = {};
    input.size = sizeof(input);
    output.size = sizeof(output);
    input.bpp = surface->bits;
    input.pitch = surface->pitch;
    input.height = surface->height;
    input.numSlices = surface->slices;
    input.numSamples = 1;
    input.tileMode = static_cast<AddrTileMode>(surface->mode);
    input.tileType = static_cast<AddrTileType>(surface->type);
    input.tileIndex = -1;
    input.pTileInfo = &tiles;
    size_t destination = 0;
    for (uint32_t z = 0; z < slices; ++z) {
        input.slice = first_slice + z;
        for (uint32_t y = 0; y < height; ++y) {
            input.y = y;
            for (uint32_t x = 0; x < width; ++x) {
                input.x = x;
                const auto status = AddrComputeSurfaceAddrFromCoord(library, &input, &output);
                if (status != ADDR_OK) return static_cast<int32_t>(status);
                if (output.bitPosition != 0 || output.addr > source_size ||
                    bytes > source_size - output.addr) return -1;
                std::memcpy(target + destination, source + static_cast<size_t>(output.addr), bytes);
                destination += bytes;
            }
        }
    }
    return 0;
}
