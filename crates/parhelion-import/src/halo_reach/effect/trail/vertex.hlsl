cbuffer Object : register(b11) { float4 objectData[4]; };
cbuffer View : register(b12) { float4 viewData[8]; };

struct Output {
    float4 t0 : TEXCOORD0;
    float3 t1 : TEXCOORD1;
    float3 t2 : TEXCOORD2;
    float4 t3 : TEXCOORD3;
    float4 t4 : TEXCOORD4;
    float4 t5 : TEXCOORD5;
    float4 t6 : TEXCOORD6;
    float4 t7 : TEXCOORD7;
    float4 t8 : TEXCOORD8;
    float2 t9 : TEXCOORD9;
    float4 position : SV_POSITION0;
};

float3 worldPoint(float3 value) {
    return value.x * objectData[0].xyz + value.y * objectData[1].xyz
        + value.z * objectData[2].xyz + objectData[3].xyz;
}

Output main(float4 v0 : TEXCOORD0, float4 v1 : TEXCOORD1,
    float4 v2 : TEXCOORD2, float4 v3 : TEXCOORD3, float4 v4 : TEXCOORD4,
    float4 v5 : TEXCOORD5, float4 v6 : TEXCOORD6, float4 v7 : TEXCOORD7,
    float4 v8 : TEXCOORD8, float4 v9 : TEXCOORD9,
    uint vertexId : SV_VERTEXID, uint instanceId : SV_InstanceID) {
    uint corner = vertexId & 3;
    float along = (corner >> 1) != 0 ? 1.0 : -1.0;
    float across = (corner & 1) != 0 ? 1.0 : 0.0;
    float centerAge = saturate(v6.x);
    float age = saturate(centerAge + along * $HALF_STEP$);

    float directionLength = dot(v2.xyz, v2.xyz);
    float3 direction = directionLength > 1e-12
        ? v2.xyz * rsqrt(directionLength) : float3(1.0, 0.0, 0.0);
    float3 tangent = direction.x * objectData[0].xyz
        + direction.y * objectData[1].xyz + direction.z * objectData[2].xyz;
    float tangentLength = dot(tangent, tangent);
    tangent = tangentLength > 1e-12
        ? tangent * rsqrt(tangentLength) : float3(1.0, 0.0, 0.0);

    // All samples in the straight trail use the same camera-facing width axis.
    // Using its origin keeps shared edges coincident even near an axial view.
    float3 origin = worldPoint(v3.xyz - direction * (centerAge * $LENGTH$));
    float3 side = cross(tangent, viewData[7].xyz - origin);
    float sideLength = dot(side, side);
    if (sideLength < 1e-8) {
        side = cross(tangent, abs(tangent.z) < 0.9
            ? float3(0.0, 0.0, 1.0) : float3(0.0, 1.0, 0.0));
        sideLength = dot(side, side);
    }
    side *= rsqrt(max(sideLength, 1e-12));
    float3 world = worldPoint(v3.xyz + direction * ((age - centerAge) * $LENGTH$))
        + side * ((1.0 - 2.0 * across) * v8.y);
    float4 projected = world.x * viewData[0] + world.y * viewData[1]
        + world.z * viewData[2] + viewData[3];
    float visible = v3.w > 0.999 ? 0.0 : 1.0;

    Output result;
    result.t0 = float4((corner >> 1), across, visible, projected.w);
    result.t1 = 0.0;
    result.t2 = viewData[7].xyz - world;
    result.t3 = v0;
    result.t4 = v1;
    result.t5 = float4(v4.xyz, v6.w);
    result.t6 = v5;
    result.t7 = float4(age, v6.y, v7.xy);
    result.t8 = float4(v7.zw, v8.xy);
    result.t9 = v8.zw;
    result.position = visible != 0.0 ? projected : float4(10000.0, 10000.0, -10000.0, 1.0);
    return result;
}
