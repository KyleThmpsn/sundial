//! Reach's atmospheric fog on a level's surfaces. Reach fogs every opaque pixel by
//! `color * extinction + inscatter` and a blended one the same way, where an additive one keeps
//! only the extinction, with `compute_scattering_core` of its `atmosphere_core.fx` shader source
//! (halohlsl/HaloReach-Shader-Source). A sky band and a ground band each fog the share of a ray
//! that lies between its floor and its top, at its rate times the square of how far below its top
//! the ray's lower end lies, over a ray shortened by the distance bias and capped at the band's
//! reach. The ground band mixes its color with the sky band's by their optical depths. Reach
//! computes this in world units. The surfaces compute it from their position and the camera's.

/// Meters per Reach world unit.
const METRES_PER_UNIT: f32 = 3.048;

/// One band of an atmosphere fog tag (`fogg`), in world units: its floor, how far above the floor
/// it reaches, its extinction per world unit, the farthest a ray travels in it, and its color
/// times its intensity.
#[derive(Clone, Copy, Debug)]
pub(in crate::halo_reach) struct Band {
    pub base: f32,
    pub height: f32,
    pub rate: f32,
    pub reach: f32,
    pub color: [f32; 3],
}

impl Band {
    /// A band whose tag `thickness` is the opacity a ray reaches at `reach`, its falloff end.
    /// Reach's shader takes an extinction rate in its place, and the conversion was not
    /// recovered. At Forge World's thickness of 0.2 a rate equal to the thickness would cover
    /// everything past 25 world units, which Reach does not draw.
    pub fn from_thickness(
        base: f32,
        height: f32,
        thickness: f32,
        reach: f32,
        color: [f32; 3],
    ) -> Self {
        let reach = reach.max(1e-3);
        Self {
            base,
            height: height.max(1e-3),
            rate: -(1. - thickness.clamp(0., 0.999)).ln() / reach,
            reach,
            color,
        }
    }

    /// A band that does not fog.
    pub fn clear() -> Self {
        Self {
            base: 0.,
            height: 1.,
            rate: 0.,
            reach: 0.,
            color: [0.; 3],
        }
    }
}

/// A level's fog: its sky and ground bands, the distance bias every ray starts after (negative
/// pushes the fog away), and how far the converted level was raised, in meters.
#[derive(Clone, Copy, Debug)]
pub(in crate::halo_reach) struct Fog {
    pub sky: Band,
    pub ground: Band,
    pub bias: f32,
    pub lift: f32,
}

impl Fog {
    /// The pixel program function `reachFog(toCamera, worldZ)`, which returns the inscatter in
    /// Reach's units and the extinction for a surface `toCamera` meters from the camera at world
    /// height `worldZ`.
    pub fn functions(&self) -> String {
        let Self { sky, ground, .. } = *self;
        // Each constant is parenthesized, since a negative one may follow a minus sign.
        let n = |value: f32| format!("({value:.9})");
        let rgb = |c: [f32; 3]| format!("float3({},{},{})", n(c[0]), n(c[1]), n(c[2]));
        format!(
            "float reachFogBand(float distance,float top,float bottom,float span,float height,float base,float rate,float reach,out float travel){{top=min(top-base,height);bottom-=base;float inside=saturate((top-bottom)/span);float weight=saturate((height-bottom)/height);travel=min(max(distance*inside+{bias},0),reach);return saturate(exp(-rate*weight*weight*travel));}}\
float4 reachFog(float3 toCamera,float worldZ){{float distance=length(toCamera)/{unit};float pointZ=(worldZ-{lift})/{unit};float viewZ=pointZ+toCamera.z/{unit};\
float top=max(viewZ,pointZ)+0.001;float bottom=min(viewZ,pointZ);float span=top-bottom;float groundTop={ground_top};float travel;\
float skyExtinction=reachFogBand(distance,top,max(bottom,groundTop),span,{sky_height},{sky_base},{sky_rate},{sky_reach},travel);\
float groundTravel;float groundExtinction=reachFogBand(distance,min(top,groundTop),bottom,span,{ground_height},{ground_base},{ground_rate},{ground_reach},groundTravel);\
float groundBottom=bottom-{ground_base};float groundDepth=-log(max(groundExtinction,0.0000001));\
float groundInside=saturate((min(min(top,groundTop)-{ground_base},{ground_height})-groundBottom)/span);float baseTravel=min(max(distance*groundInside+{bias},0),{sky_reach}-travel);\
float baseDepth={sky_rate}*baseTravel;float mixedExtinction=groundExtinction*saturate(exp(-baseDepth));\
float3 mixed=({ground_color}*groundDepth+{sky_color}*baseDepth)/max(groundDepth+baseDepth,0.00001);\
float3 skyInscatter={sky_color}*(1-skyExtinction);float3 mixedInscatter=mixed*(1-mixedExtinction);\
float low=saturate((groundTop-viewZ)/{ground_height}*{ground_rate}/max({sky_rate},0.00001));\
return float4(lerp(skyInscatter+mixedInscatter*skyExtinction,skyInscatter*mixedExtinction+mixedInscatter,low),skyExtinction*mixedExtinction);}}",
            unit = n(METRES_PER_UNIT),
            bias = n(self.bias),
            lift = n(self.lift),
            ground_top = n(ground.base + ground.height),
            sky_height = n(sky.height),
            sky_base = n(sky.base),
            sky_rate = n(sky.rate),
            sky_reach = n(sky.reach),
            ground_height = n(ground.height),
            ground_base = n(ground.base),
            ground_rate = n(ground.rate),
            ground_reach = n(ground.reach),
            sky_color = rgb(sky.color),
            ground_color = rgb(ground.color),
        )
    }
}
