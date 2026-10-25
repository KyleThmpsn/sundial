// Private flat C interface. No XG types, vtables or dynamically loaded entry points.
#pragma once
#include <cstddef>
#include <cstdint>

struct SurfaceRequest {
    uint32_t width, height, slices, bits, mode, mip, mips, base_pitch, volume;
};

struct Surface {
    uint64_t bytes;
    uint32_t pitch, height, slices, mode, type, alignment, bits;
    uint32_t banks, bank_width, bank_height, aspect, split, pipe;
};
static_assert(sizeof(SurfaceRequest) == 36, "Rust request layout");
static_assert(sizeof(Surface) == 64, "Rust surface layout");

extern "C" {
void* parhelion_addr_create();
void parhelion_addr_destroy(void* library);
int32_t parhelion_addr_surface(void* library, const SurfaceRequest* request, Surface* result);
int32_t parhelion_addr_copy(void* library, const Surface* surface,
    uint32_t width, uint32_t height, uint32_t first_slice, uint32_t slices,
    const uint8_t* source, size_t source_size, uint8_t* target, size_t target_size);
}
