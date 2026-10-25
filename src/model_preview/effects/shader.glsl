// Ports of the decoded Shadowkeep pixel forms in effects/shade.rs.
uniform int uEffect;
uniform vec4 uEffectConstants[128];
uniform sampler2D uEffectTexture0;
uniform sampler2D uEffectTexture1;
uniform sampler2D uEffectTexture2;
uniform sampler2D uSceneDepth;
uniform float uDepthScale;
in vec2 vDetailUv;

float effectRamp(float x, vec4 c) { return sat(abs(x + c.z) * c.y + c.x); }
float effectCurve(float x, float p) { return x <= 0.0 ? 0.0 : pow(x, p); }
float effectMap(float x, vec4 c) { return sat(c.z + c.w * sat(c.x + c.y * x)); }
vec2 effectUv(vec2 uv, vec4 c) { return uv * c.xy + c.zw; }
vec4 effectSurface(vec3 base, float raw, vec4 detail, vec3 dye, vec4 params, vec4 rough) {
    float mapped = effectMap(raw, rough);
    vec3 color = mix(dye, clamp(overlay3(detail.rgb, dye), 0.0, 1.0), params.x);
    return vec4(overlay3(base, color), mix(mapped, effectMap(overlay(mapped, detail.a), rough), params.z));
}
vec4 effectBase(vec2 uv) {
    vec3 base = uHasAlbedo == 1 ? samplePlate(uAlbedo, uv,0).rgb : vec3(0.0);
    vec4 mask = uHasGear == 1 ? samplePlate(uGear, uv,1) : vec4(0.0);
    if (uHasDye == 0 || mask.a < 40.0 / 255.0) return vec4(base, mask.g);
    vec4 detail = uHasDetail == 1 ? samplePlate(uDetail, effectUv(vDetailUv, uDetailTransform),3) : vec4(0.25);
    vec4 worn = effectSurface(base, mask.g, detail, uDyeWorn, clamp(uWornParams, 0.0, 1.0), uWornRough);
    vec4 fresh = effectSurface(base, mask.g, detail, uDyeAlbedo, uParams, uRough);
    return mix(worn, fresh, effectMap(sat((mask.a * 255.0 - 48.0) / 207.0), uWear));
}
vec4 scrollingEffect(vec2 uv, float facing) {
    vec2 mapped = effectUv(uv, uEffectConstants[13]);
    vec2 a = texture(uEffectTexture1, effectUv(mapped, uEffectConstants[19])).xy;
    vec2 b = texture(uEffectTexture1, effectUv(mapped, uEffectConstants[22])).xy;
    vec2 distort = effectUv(mapped.yx, uEffectConstants[18]);
    vec2 noiseUv = distort + (a - uEffectConstants[21].xy + b - uEffectConstants[23].xy) * uEffectConstants[20].x;
    float noise = texture(uEffectTexture2, noiseUv).x;
    float mask = texture(uEffectTexture0, effectUv(mapped, uEffectConstants[14])).x;
    float amount = (uEffectConstants[16].x + uEffectConstants[17].x * sat(mask * uEffectConstants[15].x + uEffectConstants[15].y)) * noise;
    amount = sat(amount * effectCurve(effectRamp(uv.x, uEffectConstants[10]), uEffectConstants[11].x) * effectRamp(uv.y, uEffectConstants[12]) * uEffectConstants[24].x + uEffectConstants[24].y);
    vec3 tint = uEffectConstants[25].rgb + uEffectConstants[26].rgb * amount;
    float edge = effectRamp(uv.x, uEffectConstants[27]) * effectRamp(uv.y, uEffectConstants[28]);
    vec3 highlight = uEffectConstants[29].rgb + uEffectConstants[30].rgb * edge * edge;
    vec3 sparkle = texture(uEffectTexture1, effectUv(mapped, uEffectConstants[32])).rgb;
    tint += highlight * sparkle * effectRamp(mapped.y, uEffectConstants[31]);
    tint *= uEffectConstants[8].rgb + uEffectConstants[9].rgb * effectRamp(uv.y, uEffectConstants[7]);
    float angle = min(effectCurve(sat(facing * uEffectConstants[3].x + uEffectConstants[3].y), uEffectConstants[4].x), 1.0);
    tint *= (uEffectConstants[5].rgb + uEffectConstants[6].rgb * angle) * uEffectConstants[33].rgb;
    vec3 plate = uHasAlbedo == 1 ? samplePlate(uAlbedo, uv,0).rgb : vec3(0.0);
    tint = (tint * plate.r + uEffectConstants[34].rgb * (uEffectConstants[35].rgb * plate.g + uEffectConstants[36].rgb * plate.b)) * uEffectConstants[37].x;
    float alpha = (uHasGear == 1 ? sat(samplePlate(uGear, uv,1).g) : 0.0) * uEffectConstants[38].x;
    return vec4(tint * uEffectConstants[38].x * alpha * uEffectConstants[40].rgb, alpha * uEffectConstants[39].x);
}
vec4 distortedGlow(vec2 uv, float facing) {
    float angle = 1.0 - pow(1.0 - abs(facing), 2.0);
    vec4 tint = uEffectConstants[8] + uEffectConstants[10] * sat(angle * uEffectConstants[9].x + uEffectConstants[9].y);
    vec2 distortion = texture(uEffectTexture0, effectUv(uv, uEffectConstants[1])).xy;
    vec2 noisy = effectUv(uv, uEffectConstants[0]) + distortion * uEffectConstants[2].xy + uEffectConstants[2].zw;
    vec4 sampled = texture(uEffectTexture1, noisy) * uEffectConstants[4].x + uEffectConstants[3];
    float fade = effectRamp(uv.x, uEffectConstants[5]) * effectRamp(uv.y, uEffectConstants[6]);
    vec4 rgba = sampled * fade * fade * sat(uEffectConstants[7].x) * tint;
    return vec4(rgba.rgb * rgba.a * uEffectConstants[12].x * uEffectConstants[11].rgb, 0.0);
}
vec4 waveGlow(vec2 uv, float facing) {
    float outer = min(effectCurve(sat(facing * uEffectConstants[3].x + uEffectConstants[3].y), uEffectConstants[4].x) * effectRamp(uv.y, uEffectConstants[2]) * effectRamp(uv.x, uEffectConstants[1]), 1.0);
    float shape = effectCurve(effectRamp(uv.y, uEffectConstants[13]) * effectRamp(uv.y, uEffectConstants[14]) * outer, uEffectConstants[15].x);
    vec2 mapped = effectUv(uv, uEffectConstants[0]);
    float noise = texture(uEffectTexture0, effectUv(mapped, uEffectConstants[7])).r * texture(uEffectTexture0, effectUv(mapped, uEffectConstants[8])).r * 4.594793 + texture(uEffectTexture0, effectUv(mapped, uEffectConstants[6])).r;
    float level = uEffectConstants[10].x + uEffectConstants[10].y * effectRamp(uv.y, uEffectConstants[9]) + noise;
    level = mix(uEffectConstants[5].x, level, sat(uEffectConstants[11].x));
    level = uEffectConstants[12].x + uEffectConstants[12].y * level;
    float alpha = sat(shape * level * uEffectConstants[16].x + uEffectConstants[16].y);
    vec4 first = vec4((uEffectConstants[18].rgb + uEffectConstants[19].rgb * effectRamp(uv.y, uEffectConstants[17])) * alpha, alpha);
    float mask = texture(uEffectTexture1, effectUv(mapped, uEffectConstants[21])).r * texture(uEffectTexture1, effectUv(mapped, uEffectConstants[20])).r;
    mask *= effectRamp(uv.y, uEffectConstants[22]) * effectRamp(uv.y, uEffectConstants[23]);
    mask = sat(effectCurve(sat(mask * uEffectConstants[24].x + uEffectConstants[24].y), uEffectConstants[25].x) * uEffectConstants[26].x);
    vec4 rgba = (first * 4.594793 + mask * uEffectConstants[27] * uEffectConstants[28]) * uEffectConstants[29].x * outer;
    return vec4(rgba.rgb * rgba.a * uEffectConstants[31].x * uEffectConstants[30].rgb, 0.0);
}
vec4 effectColor(vec2 uv, vec3 normal) {
    float facing = normal.z * normal.z;
    float gap = max(0.0, (texelFetch(uSceneDepth, ivec2(gl_FragCoord.xy), 0).r - gl_FragCoord.z) * 2.0 / uDepthScale);
    vec4 outputColor = vec4(0.0);
    if (uEffect == 1) {
        float fade = sat(facing * uEffectConstants[4].x + uEffectConstants[4].y);
        fade = fade * fade * effectCurve(effectRamp(uv.y, uEffectConstants[5]), uEffectConstants[6].x);
        float t = min(effectCurve(effectRamp(uv.y, uEffectConstants[0]), uEffectConstants[1].x), 1.0);
        vec4 rgba = (uEffectConstants[2] + uEffectConstants[3] * t) * fade;
        outputColor.rgb = rgba.rgb * rgba.a * uEffectConstants[8].x * uEffectConstants[7].rgb;
    } else if (uEffect == 2) {
        float t = min(effectCurve(effectRamp(uv.y, uEffectConstants[1]), uEffectConstants[2].x), 1.0);
        vec3 tint = uEffectConstants[3].rgb + uEffectConstants[4].rgb * t;
        float angle = sat(facing * uEffectConstants[0].x + uEffectConstants[0].y);
        float fade = sat(gap * uEffectConstants[7].x + uEffectConstants[7].y) * effectRamp(uv.y, uEffectConstants[5]) * effectRamp(uv.x, uEffectConstants[6]);
        fade *= fade;
        outputColor.rgb = tint * angle * angle * fade * fade * uEffectConstants[9].x * uEffectConstants[8].rgb;
    } else if (uEffect == 3) {
        float mask = uHasGear == 1 ? samplePlate(uGear, uv,1).g : 0.0;
        vec4 extra = uEffectConstants[4] + uEffectConstants[5] * sat((1.0 - mask) * uEffectConstants[3].x + uEffectConstants[3].y);
        vec4 rgba = (effectBase(uv) + extra) * uEffectConstants[6].x;
        outputColor.rgb = rgba.rgb * rgba.a * uEffectConstants[7].rgb;
    } else if (uEffect == 4) {
        float mask = uHasGear == 1 ? samplePlate(uGear, uv,1).g : 0.0;
        vec4 a = uEffectConstants[5] + uEffectConstants[6] * min(effectCurve(sat(facing * uEffectConstants[3].x + uEffectConstants[3].y), uEffectConstants[4].x), 1.0);
        vec4 b = uEffectConstants[9] + uEffectConstants[10] * min(effectCurve(sat(facing * uEffectConstants[7].x + uEffectConstants[7].y), uEffectConstants[8].x), 1.0);
        vec4 tint = uEffectConstants[15] + uEffectConstants[16] * min(effectCurve(sat(facing * uEffectConstants[13].x + uEffectConstants[13].y), uEffectConstants[14].x), 1.0);
        float amount = uEffectConstants[12].x + uEffectConstants[12].y * sat(mask * uEffectConstants[11].x + uEffectConstants[11].y);
        float fade = effectCurve(sat(gap * uEffectConstants[17].x + uEffectConstants[17].y), uEffectConstants[18].x);
        vec4 rgba = (effectBase(uv) + (a + b * amount) * tint * fade) * uEffectConstants[19].x;
        outputColor.rgb = rgba.rgb * rgba.a * uEffectConstants[20].rgb;
    } else if (uEffect == 5) {
        outputColor = scrollingEffect(uv, facing);
    } else if (uEffect == 6) {
        outputColor = distortedGlow(uv, normal.z);
    } else if (uEffect == 7) {
        outputColor = waveGlow(uv, facing);
    }
    outputColor.rgb = max(outputColor.rgb * uExposure, vec3(0.0));
    outputColor.a = sat(outputColor.a);
    return outputColor;
}
