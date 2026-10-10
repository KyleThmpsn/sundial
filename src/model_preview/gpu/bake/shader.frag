#version 330 core
// Static glTF material semantics. No preview lighting or view-dependent iridescence.
uniform ivec2 uSize;
uniform int uRow,uCell,uCount,uSlot;
uniform samplerBuffer uSurfaces;
uniform usampler2D uOwner;
uniform sampler2D uAlbedo,uGear,uNormal,uDetail,uDetailNormal,uDyeMap;
uniform int uHasAlbedo,uHasGear,uHasNormal,uHasDetail,uHasDetailNormal,uHasDyeMap,uHasDye,uSkipNormal;
uniform vec4 uMapTransform,uDetailTransform,uNormalTransform;
uniform vec3 uDyeAlbedo,uDyeWorn,uEmissive;
uniform vec4 uParams,uWornParams,uRough,uWornRough,uWear;
uniform int uHasPaint,uHasGain,uHasMetal,uHasDecode,uLegacy,uHasGrain;
uniform vec2 uPaint,uLegacyOffsets;
uniform vec3 uGain;
uniform vec4 uDecode;
uniform float uMetal,uGrain;
layout(location=0) out vec4 color;
layout(location=1) out vec4 channels;
layout(location=2) out vec4 normal;
layout(location=3) out vec4 emissionOut;
float sat(float x){return clamp(x,0.0,1.0);}
float remap(float x,vec4 m){return sat(m.z+m.w*sat(m.x+m.y*x));}
float overlay(float a,float b){return b*sat(a*4.0)+sat(a-0.25);}
vec3 overlay3(vec3 a,vec3 b){return vec3(overlay(a.r,b.r),overlay(a.g,b.g),overlay(a.b,b.b));}
vec3 legacyColor(vec3 base,vec3 detail,vec3 dye,float strength){return overlay3(base,mix(dye,clamp(overlay3(detail,dye),0.0,1.0),strength));}
vec3 paintColor(vec3 base,vec3 detail,vec3 dye,float strength){return clamp(legacyColor(base,detail,dye,sat(strength)),0.0,1.0);}
float paintSmooth(float raw,float detail,vec4 map,float strength){return mix(remap(raw,map),remap(sat(overlay(raw,detail)),map),sat(strength));}
bool finite2(vec2 v){return !any(isnan(v))&&!any(isinf(v));}
vec3 unit(vec3 v){float length2=dot(v,v);return length2>1e-12 ? v*inversesqrt(length2):vec3(0.0,0.0,1.0);}
void main(){
    ivec2 pixel=ivec2(gl_FragCoord.xy)+ivec2(0,uRow);
    int index=uCell==0 ? int(texelFetch(uOwner,pixel,0).r)-1 : (pixel.y/uCell)*(uSize.x/uCell)+pixel.x/uCell;
    if(index<0||index>=uCount)discard;
    vec4 surface=texelFetch(uSurfaces,index*6);
    if(int(surface.x)!=uSlot)discard;
    vec2 uv=(vec2(pixel)+0.5)/vec2(uSize);
    vec2 coordinates=uv*5.0;
    if(uCell>0){
        vec2 weight=clamp((vec2(pixel%uCell)-4.0)/float(uCell-9),0.0,1.0);
        float sum=weight.x+weight.y;
        if(sum>1.0)weight/=sum;
        vec4 a=texelFetch(uSurfaces,index*6+3),b=texelFetch(uSurfaces,index*6+4),c=texelFetch(uSurfaces,index*6+5);
        vec4 mapped=a+weight.x*(b-a)+weight.y*(c-a);
        uv=mapped.xy;coordinates=mapped.zw;
    }else if(surface.z>0.5){
        vec3 placed=vec3(uv,1.0);
        coordinates=vec2(dot(texelFetch(uSurfaces,index*6+1).xyz,placed),dot(texelFetch(uSurfaces,index*6+2).xyz,placed));
    }
    vec3 base=textureLod(uAlbedo,uv,0.0).rgb;
    vec4 mask=uHasGear==1 ? textureLod(uGear,uv,0.0)*255.0:vec4(0.0);
    bool dyed=uHasDye==1&&uHasGear==1;
    bool material=dyed||uHasNormal==1||(uHasPaint==1&&uHasGear==1);
    vec3 albedo=base,emission=vec3(0.0);
    float rough=0.6,metal=0.0,ao=1.0;
    float intact=remap(sat((mask.a-48.0)/207.0),uWear);
    vec2 detailUv=coordinates*uDetailTransform.xy+uDetailTransform.zw;
    vec2 normalUv=coordinates*uNormalTransform.xy+uNormalTransform.zw;
    if(dyed){
        rough=1.0-mask.g/255.0;metal=sat(mask.a/32.0);ao=sat(mask.r/255.0);
        emission=base*sat((mask.b-40.0)/215.0);
        vec4 detail=uHasDetail==1 ? textureLod(uDetail,detailUv,0.0):vec4(0.25);
        if(mask.a>=40.0){
            vec4 params=mix(uWornParams,uParams,intact);
            albedo=mix(legacyColor(base,detail.rgb,uDyeWorn,sat(uWornParams.x)),legacyColor(base,detail.rgb,uDyeAlbedo,uParams.x),intact);
            float gloss=mask.g/255.0;
            if(uHasDetail==1)gloss=mix(gloss,overlay(gloss,detail.a),sat(params.z));
            rough=1.0-sat(mix(remap(gloss,uWornRough),remap(gloss,uRough),intact));
            metal=sat(params.w);emission=uEmissive*sat((mask.b-40.0)/215.0);
            if(uHasPaint==1){
                albedo=mix(paintColor(base,detail.rgb,uDyeWorn,uWornParams.x),paintColor(base,detail.rgb,uDyeAlbedo,uParams.x),intact);
                rough=1.0-mix(paintSmooth(uPaint.x,detail.a,uWornRough,uWornParams.z),paintSmooth(uPaint.x,detail.a,uRough,uParams.z),intact);
                metal=mix(sat(uWornParams.w),sat(uParams.w),intact);
            }
        }else if(uHasPaint==1){rough=1.0-uPaint.y;}
    }
    vec2 encodedNormal=vec2(0.5);
    bool hasNormal=false;
    if(material){
        if(uHasPaint==0&&all(equal(emission,vec3(0.0)))){
            albedo*=1.0-sat(max(0.0,max(albedo.r,max(albedo.g,albedo.b)))-1.0);
            albedo/=max(1.0,max(albedo.r,max(albedo.g,albedo.b)));
        }
        if(uHasGear==1&&mask.a<40.0){
            if(uHasGain==1)albedo*=uGain;
            if(uHasMetal==1)metal=uMetal;
            if(uHasPaint==1)rough=1.0-uPaint.y;
        }
        vec4 sampled=uHasNormal==1 ? textureLod(uNormal,uv,0.0):vec4(0.0);
        vec4 detail=uHasDetailNormal==1 ? textureLod(uDetailNormal,normalUv,0.0):vec4(0.0);
        float decodedStrength=mix(sat(uWornParams.y),uLegacy==1?uParams.y:sat(uParams.y),intact);
        if(uLegacy==1&&uHasNormal==1){
            float limit=sat(sampled.b+uLegacyOffsets.x);
            if(dyed&&uHasDetailNormal==1&&!isnan(uLegacyOffsets.y)&&!isinf(uLegacyOffsets.y))limit=min(limit,mix(1.0,sat(detail.b+uLegacyOffsets.y),decodedStrength));
            rough=max(rough,1.0-limit);
        }else if(uHasGrain==1&&dyed&&uHasDetailNormal==1&&mask.a>=40.0){
            float strength=mix(sat(uWornParams.y),sat(uParams.y),intact);
            rough=max(rough,1.0-mix(1.0,sat(detail.b+uGrain),strength));
        }
        if(uHasNormal==1&&uSkipNormal==0){
            hasNormal=true;
            encodedNormal=sampled.rg;
            if(uHasDecode==1){
                vec2 xy=sampled.rg*uDecode.x+uDecode.y;
                if(dyed&&uHasDetailNormal==1&&mask.a>=40.0&&finite2(uDecode.zw))xy+=decodedStrength*(detail.rg*uDecode.z+uDecode.w);
                encodedNormal=unit(vec3(xy,sqrt(max(0.0,1.0-dot(xy,xy))))).xy*0.5+0.5;
            }else{
                ao*=sampled.b;
                if(dyed&&uHasDetailNormal==1&&mask.a>=40.0){
                    float strength=clamp(mix(uWornParams.y,uParams.y,intact),0.0,4.0);
                    vec2 blended=mix(2.0*encodedNormal*detail.rg,1.0-2.0*(1.0-encodedNormal)*(1.0-detail.rg),step(vec2(0.5),encodedNormal));
                    encodedNormal=mix(encodedNormal,blended,strength);
                    ao*=mix(1.0,detail.b,min(strength,1.0));
                }
            }
        }
    }else if(uHasDye==1){albedo*=uDyeAlbedo;}
    float alpha=1.0;
    if(surface.y>0.5||uHasDyeMap==1){
        // Dye-bank membership uses the stored map and the native cutoff before normalization.
        bool covered=true;
        float coverage=sat(mask.b*7.96875/255.0);
        if(uHasDyeMap==1){
            vec3 map=textureLod(uDyeMap,uv*uMapTransform.xy+uMapTransform.zw,0.0).rgb;
            float third=map.g-map.b<1.2/255.0?map.g:map.b;
            int bank=third>=0.5?2:(map.g>=0.5?1:0);
            covered=bank*2+(map.r>=0.5?1:0)==uSlot;
            if(uHasGear==1&&surface.y>0.5)covered=covered&&coverage>=surface.w;
        }
        alpha=covered?(uHasGear==1&&surface.y>0.5?sat(coverage*0.5/surface.w):1.0):0.0;
    }
    color=vec4(albedo,alpha);
    channels=vec4(ao,rough,metal,1.0);
    vec2 xy=clamp(encodedNormal*2.0-1.0,-1.0,1.0);
    normal=vec4(hasNormal?unit(vec3(xy.x,-xy.y,sqrt(max(0.0,1.0-dot(xy,xy)))))*0.5+0.5:vec3(128.0/255.0,128.0/255.0,1.0),1.0);
    emissionOut=vec4(emission,1.0);
}
