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

Output main(float4 a0 : TEXCOORD0, float4 a1 : TEXCOORD1,
    float4 a2 : TEXCOORD2, float4 a3 : TEXCOORD3,
    float4 a4 : TEXCOORD4, float4 a5 : TEXCOORD5,
    float4 a6 : TEXCOORD6, float4 a7 : TEXCOORD7, uint vertexId : SV_VERTEXID) {
    uint corner = vertexId & 3;
    float along = (corner >> 1) != 0 ? 1.0 : -1.0;
    float across = (corner & 1) != 0 ? 1.0 : 0.0;
    float3 center = a2.x * objectData[0].xyz + a2.y * objectData[1].xyz
        + a2.z * objectData[2].xyz + objectData[3].xyz;
    float3 eye = viewData[7].xyz - center;
    float eyeLength = dot(eye, eye);
    float3 facing = eyeLength > 1e-12 ? eye * rsqrt(eyeLength) : float3(0, 0, 1);
    float3 right = cross(facing, viewData[4].xyz);
    float rightLength = dot(right, right);
    if (rightLength < 1e-8) {
        right = cross(facing, abs(facing.z) < 0.9 ? float3(0, 0, 1) : float3(0, 1, 0));
        rightLength = dot(right, right);
    }
    right *= rsqrt(max(rightLength, 1e-12));
    float3 up = cross(right, facing);
    float age = saturate(a1.w + along * $HALF_STEP$);
    float3 world = center + right * (along * $HALF_LENGTH$)
        + up * ((1.0 - 2.0 * across) * a3.y);
    float4 projected = world.x * viewData[0] + world.y * viewData[1]
        + world.z * viewData[2] + viewData[3];
    float visible = a1.w < 1.0 ? 1.0 : 0.0;
    Output result = (Output)0;
    result.t0 = float4((corner >> 1), across, visible, projected.w);
    result.t2 = eye;
    result.t7 = float4(age, 0, 0, 0);
    result.position = visible != 0.0 ? projected : float4(10000, 10000, -10000, 1);
    return result;
}
