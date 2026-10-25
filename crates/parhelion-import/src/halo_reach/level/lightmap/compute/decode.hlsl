// MCC BC3 intensity layers and unsigned BC5 direction data. Integer interpolation
// retains the decoder's endpoint precision before the Reach lighting equations.
StructuredBuffer<uint4> intensity0 : register(t0);
StructuredBuffer<uint4> intensity1 : register(t1);
StructuredBuffer<uint4> intensity2 : register(t2);
StructuredBuffer<uint4> directionMap : register(t3);
cbuffer Parameters : register(b0) { uint width; uint height; float brightness; uint unused; };
struct Texel { float3 lobe; float3 ambient; uint directionBandwidth; uint sun; };
RWStructuredBuffer<Texel> output : register(u0);

uint selector(uint2 block, uint texel) {
    uint shift = 16 + texel * 3;
    return (shift < 32 ? ((block.x >> shift) | (block.y << (32 - shift)))
                       : (block.y >> (shift - 32))) & 7;
}

uint alphaByte(uint2 block, uint texel) {
    uint a = block.x & 255, b = (block.x >> 8) & 255;
    uint s = selector(block, texel);
    if (s == 0) return a;
    if (s == 1) return b;
    if (a > b) return ((8 - s) * a + (s - 1) * b + 1) / 7;
    if (s == 6) return 0;
    if (s == 7) return 255;
    return ((6 - s) * a + (s - 1) * b + 1) / 5;
}

float alphaFloat(uint2 block, uint texel) {
    precise float a = float(block.x & 255) / 255.0;
    precise float b = float((block.x >> 8) & 255) / 255.0;
    uint s = selector(block, texel);
    if (s == 0) return a;
    if (s == 1) return b;
    if (a > b) return (float(8 - s) * a + float(s - 1) * b) / 7.0;
    if (s == 6) return 0.0;
    if (s == 7) return 1.0;
    return (float(6 - s) * a + float(s - 1) * b) / 5.0;
}

uint4 bc3(uint4 block, uint texel) {
    uint c0 = block.z & 65535, c1 = block.z >> 16;
    uint3 a = uint3((c0 >> 11) & 31, (c0 >> 5) & 63, c0 & 31);
    uint3 b = uint3((c1 >> 11) & 31, (c1 >> 5) & 63, c1 & 31);
    uint s = (block.w >> (texel * 2)) & 3;
    uint3 color;
    if (s < 2) {
        uint3 c = s == 0 ? a : b;
        color = (c * uint3(527, 259, 527) + uint3(23, 33, 23)) >> 6;
    } else {
        uint3 c = s == 2 ? 2 * a + b : a + 2 * b;
        color = (c * uint3(351, 2763, 351) + uint3(61, 1039, 61)) >> uint3(7, 11, 7);
    }
    return uint4(color, alphaByte(block.xy, texel));
}

uint byteValue(float value) { return uint(saturate(value) * 255.0 + 0.5); }

[numthreads(64, 1, 1)]
void main(uint3 thread : SV_DispatchThreadID) {
    uint index = thread.x;
    if (index >= width * height) return;
    uint x = index % width, y = index / width;
    uint block = (y / 4) * (width / 4) + x / 4;
    uint texel = (y % 4) * 4 + x % 4;
    precise float4 a = float4(bc3(intensity0[block], texel)) / 255.0;
    precise float4 b = float4(bc3(intensity1[block], texel)) / 255.0;
    precise float4 c = float4(bc3(intensity2[block], texel)) / 255.0;
    uint4 map = directionMap[block];
    precise float exposure = exp2(-9.000001 * alphaFloat(map.xy, texel));
    precise float3 axis = float3(a.a, b.a, c.a) * 2.0 - 1.0;
    precise float length = sqrt(axis.x * axis.x + axis.y * axis.y + axis.z * axis.z);
    precise float3 direction = length > 0.00001 ? axis / length : float3(0, 0, 1);
    Texel result;
    result.lobe = max((a.rgb + 2.0 * b.rgb - 1.0) * exposure, 0.0) * brightness;
    result.ambient = 0.25 * c.rgb * exposure * brightness;
    result.directionBandwidth = byteValue(direction.x * 0.5 + 0.5)
        | (byteValue(direction.y * 0.5 + 0.5) << 8)
        | (byteValue(direction.z * 0.5 + 0.5) << 16)
        | (byteValue(length) << 24);
    result.sun = byteValue(alphaFloat(map.zw, texel));
    output[index] = result;
}
