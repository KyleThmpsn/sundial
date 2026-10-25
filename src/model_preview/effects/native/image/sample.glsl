vec4 nImageFetch(sampler2D image,int level,ivec2 size,ivec2 at,ivec2 modes,vec4 border){
    at=ivec2(nLayerAddress(at.x,size.x,modes.x),nLayerAddress(at.y,size.y,modes.y));
    if(any(lessThan(at,ivec2(0))))return border;
    return texelFetch(image,at,level);
}
vec4 nImageLevel(sampler2D image,int level,vec2 uv,ivec2 offset,ivec2 modes,vec4 border,bool linear){
    if(any(isnan(uv))||any(isinf(uv)))return border;
    ivec2 size=textureSize(image,level);
    vec2 p=vec2(nLayerPosition(uv.x+float(offset.x)/float(size.x),size.x,modes.x),nLayerPosition(uv.y+float(offset.y)/float(size.y),size.y,modes.y));
    ivec2 at=ivec2(floor(p));vec2 t=fract(p);
    if(!linear)return nImageFetch(image,level,size,at+ivec2(step(vec2(0.5),t)),modes,border);
    vec4 result=vec4(0.0);
    for(int y=0;y<2;y++)for(int x=0;x<2;x++){
        float weight=(x==0?1.0-t.x:t.x)*(y==0?1.0-t.y:t.y);
        result+=nImageFetch(image,level,size,at+ivec2(x,y),modes,border)*weight;
    }
    return result;
}
