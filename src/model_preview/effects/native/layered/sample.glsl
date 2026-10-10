ivec3 nLayerSize(ivec3 size,int mip,bool volume){
    return ivec3(max(size.xy>>mip,ivec2(1)),volume?max(size.z>>mip,1):size.z);
}
ivec2 nLayerOrigin(ivec3 size,int mip,int slice,int columns,bool volume){
    int top=0;
    for(int i=0;i<mip;i++){ivec3 s=nLayerSize(size,i,volume);top+=s.y*((s.z+columns-1)/columns);}
    return ivec2((slice%columns)*size.x,top+(slice/columns)*nLayerSize(size,mip,volume).y);
}
vec4 nLayerLoad(sampler2D atlas,ivec3 size,int levels,int columns,bool volume,ivec4 p,ivec3 offset){
    p.xy+=offset.xy;if(volume)p.z+=offset.z;
    if(p.w<0||p.w>=levels)return vec4(0.0);
    ivec3 s=nLayerSize(size,p.w,volume);
    if(any(lessThan(p.xyz,ivec3(0)))||any(greaterThanEqual(p.xyz,s)))return vec4(0.0);
    return texelFetch(atlas,nLayerOrigin(size,p.w,p.z,columns,volume)+p.xy,0);
}
float nLayerPosition(float uv,int size,int mode){
    if(mode==1)uv=mod(uv,1.0);
    else if(mode==2)uv=mod(uv,2.0);
    else if(mode==3)uv=clamp(uv,0.0,1.0);
    else if(mode==4)uv=clamp(uv,-1.0,2.0);
    else uv=clamp(abs(uv),0.0,1.0);
    return uv*float(size)-0.5;
}
int nLayerAddress(int value,int size,int mode){
    if(mode==1)return ((value%size)+size)%size;
    if(mode==2){int v=((value%(size*2))+size*2)%(size*2);return v<size?v:size*2-1-v;}
    if(mode==3||mode==5)return clamp(value,0,size-1);
    return value>=0&&value<size?value:-1;
}
vec4 nLayerFetch(sampler2D atlas,ivec3 base,int level,int columns,bool volume,ivec3 p,ivec3 modes,vec4 border){
    ivec3 size=nLayerSize(base,level,volume);
    for(int i=0;i<3;i++)p[i]=nLayerAddress(p[i],size[i],i==2&&!volume?3:modes[i]);
    if(any(lessThan(p,ivec3(0))))return border;
    return texelFetch(atlas,nLayerOrigin(base,level,p.z,columns,volume)+p.xy,0);
}
vec4 nLayerLevel(sampler2D atlas,ivec3 base,int level,int columns,bool volume,vec3 uv,ivec3 offset,ivec3 modes,vec4 border,bool linear){
    if(any(isnan(uv))||any(isinf(uv)))return border;
    ivec3 size=nLayerSize(base,level,volume);vec3 p;
    for(int i=0;i<3;i++)p[i]=i==2&&!volume?clamp(roundEven(uv.z),0.0,float(size.z-1)):nLayerPosition(uv[i]+float(offset[i])/float(size[i]),size[i],modes[i]);
    ivec3 at=ivec3(floor(p));vec3 t=fract(p);
    if(!linear)return nLayerFetch(atlas,base,level,columns,volume,at+ivec3(step(vec3(0.5),t)),modes,border);
    vec4 result=vec4(0.0);
    for(int z=0;z<2;z++)for(int y=0;y<2;y++)for(int x=0;x<2;x++){
        if(!volume&&z!=0)continue;
        float weight=(x==0?1.0-t.x:t.x)*(y==0?1.0-t.y:t.y)*(volume?(z==0?1.0-t.z:t.z):1.0);
        result+=nLayerFetch(atlas,base,level,columns,volume,at+ivec3(x,y,z),modes,border)*weight;
    }
    return result;
}
float nLayerLod(ivec3 size,bool volume,vec3 dx,vec3 dy){
    if(!volume){dx.z=0.0;dy.z=0.0;}
    return log2(max(max(length(dx*vec3(size)),length(dy*vec3(size))),1e-20));
}
vec4 nLayerSample(sampler2D atlas,ivec3 size,int levels,int columns,bool volume,vec3 uv,float lod,ivec3 offset,ivec3 modes,vec4 border,int filtering,vec2 limits){
    float raw=clamp(lod,limits.x,limits.y);lod=clamp(raw,0.0,float(levels-1));
    if(isnan(lod)||isinf(lod))lod=0.0;
    bool linear=(filtering&(raw<=0.0?4:16))!=0;
    int low=int((filtering&1)!=0?floor(lod):floor(lod+0.5));
    int high=int((filtering&1)!=0?ceil(lod):floor(lod+0.5));
    return mix(nLayerLevel(atlas,size,low,columns,volume,uv,offset,modes,border,linear),nLayerLevel(atlas,size,high,columns,volume,uv,offset,modes,border,linear),(filtering&1)!=0?fract(lod):0.0);
}
