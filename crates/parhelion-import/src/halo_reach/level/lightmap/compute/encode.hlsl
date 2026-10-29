struct Texel { float3 lobe; float3 ambient; uint directionBandwidth; uint sun; };
struct Encoded { uint lobe; uint ambient; uint direction; };
StructuredBuffer<Texel> input : register(t0);
RWStructuredBuffer<Encoded> output : register(u0);
cbuffer Parameters : register(b0) { uint count; float peak; float range; uint unused; };

// The logarithm of a color's share of the peak, as `lightmap::encode_share` takes it in the same
// order: log2(1 + range * share) / log2(1 + range).
uint color(float3 value) {
    precise float3 normalized = saturate(value / peak);
    precise float3 stored = log2(1.0 + normalized * range) / log2(1.0 + range);
    uint3 bytes = uint3(stored * 255.0 + 0.5);
    return bytes.x | (bytes.y << 8) | (bytes.z << 16);
}

[numthreads(64, 1, 1)]
void main(uint3 thread : SV_DispatchThreadID) {
    uint index = thread.x;
    if (index >= count) return;
    Texel texel = input[index];
    Encoded result;
    result.lobe = color(texel.lobe) | (texel.directionBandwidth & 0xff000000);
    result.ambient = color(texel.ambient) | ((texel.sun & 255) << 24);
    result.direction = (texel.directionBandwidth & 0x00ffffff) | 0xff000000;
    output[index] = result;
}
