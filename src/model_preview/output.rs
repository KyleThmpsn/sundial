//! Shadowkeep output arithmetic with fixed studio exposure and a neutral color grade.
//! Bloom uses verified native kernels at three scales. Its composition is viewer policy,
//! without world lens dirt, distortion, adaptation or a selected environment's color LUT.
use super::{
    render::{Scene, Style},
    shader,
};

pub(super) fn film(value: f32) -> f32 {
    let x = value.max(0.0);
    let a = ((4.016 * x + 0.030) * (1.6 * x) / ((3.888 * x + 0.590) * (1.6 * x) + 0.140))
        .clamp(0.0, 1.0);
    let b = (x * (1.048_747 * x + 3.134_397) / (x * (0.990_440 * x + 3.240_45) + 0.651_790))
        .clamp(0.0, 1.0);
    0.4 * a + 0.6 * b
}

pub(super) fn background(scene: Scene, style: Style) -> [f32; 3] {
    scene.background.map(|v| {
        let linear = shader::linear(f32::from(v) / 255.0);
        if !scene.filmic || style != Style::Textured {
            return linear;
        }
        // Preserve the viewer's chosen backdrop through the output curve.
        let (mut low, mut high) = (0.0, 64.0);
        for _ in 0..30 {
            let middle = (low + high) * 0.5;
            if film(middle) < linear {
                low = middle;
            } else {
                high = middle;
            }
        }
        (low + high) * 0.5
    })
}

fn bright(rgb: [f32; 3]) -> [f32; 3] {
    let luminance = rgb[0] * 0.3 + rgb[1] * 0.59 + rgb[2] * 0.11;
    rgb.map(|v| v * (0.016 + 0.0005 * luminance))
}

struct Image {
    size: [usize; 2],
    values: Vec<[f32; 3]>,
}

fn sample(values: &[[f32; 3]], size: [usize; 2], uv: [f32; 2]) -> [f32; 3] {
    let point = std::array::from_fn::<_, 2, _>(|i| uv[i] * size[i] as f32 - 0.5);
    let base = point.map(|v| v.floor());
    let fraction = std::array::from_fn::<_, 2, _>(|i| point[i] - base[i]);
    let mut result = [0.0; 3];
    for y in 0..2 {
        for x in 0..2 {
            let weight = (if x == 0 {
                1.0 - fraction[0]
            } else {
                fraction[0]
            }) * (if y == 0 {
                1.0 - fraction[1]
            } else {
                fraction[1]
            });
            let at = [x, y].map(|i| i as f32);
            let index = std::array::from_fn::<_, 2, _>(|i| {
                (base[i] + at[i]).clamp(0.0, (size[i] - 1) as f32) as usize
            });
            let pixel = values[index[1] * size[0] + index[0]];
            for lane in 0..3 {
                result[lane] += pixel[lane] * weight;
            }
        }
    }
    result
}

fn downsample(values: &[[f32; 3]], size: [usize; 2], backdrop: Option<[f32; 3]>) -> Image {
    let output = size.map(|v| v.div_ceil(2));
    let mut pixels = Vec::with_capacity(output[0] * output[1]);
    for y in 0..output[1] {
        for x in 0..output[0] {
            let center = [
                (x as f32 + 0.5) / output[0] as f32,
                (y as f32 + 0.5) / output[1] as f32,
            ];
            let mut rgb = [0.0; 3];
            for offset in [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]] {
                let point = std::array::from_fn(|i| center[i] + offset[i] / size[i] as f32);
                let pixel = sample(values, size, point);
                for lane in 0..3 {
                    rgb[lane] += pixel[lane].clamp(0.0, 65_504.0) * 0.25;
                }
            }
            if let Some(backdrop) = backdrop {
                let base = bright(backdrop);
                rgb = std::array::from_fn(|i| (bright(rgb)[i] - base[i]).max(0.0));
            }
            pixels.push(rgb);
        }
    }
    Image {
        size: output,
        values: pixels,
    }
}

fn blur(image: &mut Image, vertical: bool) {
    let mut pixels = Vec::with_capacity(image.values.len());
    for y in 0..image.size[1] {
        for x in 0..image.size[0] {
            let center = [
                (x as f32 + 0.5) / image.size[0] as f32,
                (y as f32 + 0.5) / image.size[1] as f32,
            ];
            let mut rgb = [0.0; 3];
            for (offset, weight) in [
                (-4.5, 0.05882),
                (-7.0 / 3.0, 0.17647),
                (0.0, 0.52941),
                (7.0 / 3.0, 0.17647),
                (4.5, 0.05882),
            ] {
                let mut uv = center;
                let axis = usize::from(vertical);
                uv[axis] += offset / image.size[axis] as f32;
                let pixel = sample(&image.values, image.size, uv);
                for lane in 0..3 {
                    rgb[lane] += pixel[lane] * weight;
                }
            }
            pixels.push(rgb);
        }
    }
    image.values = pixels;
}

fn bloom(pixels: &mut [[f32; 3]], size: [usize; 2], scene: Scene) {
    let first = downsample(pixels, size, Some(background(scene, Style::Textured)));
    let second = downsample(&first.values, first.size, None);
    let third = downsample(&second.values, second.size, None);
    let mut levels = [first, second, third];
    for image in &mut levels {
        blur(image, false);
        blur(image, true);
    }
    for (index, pixel) in pixels.iter_mut().enumerate() {
        let uv = [
            (index % size[0]) as f32 + 0.5,
            (index / size[0]) as f32 + 0.5,
        ];
        let uv = std::array::from_fn(|i| uv[i] / size[i] as f32);
        for image in &levels {
            let glow = sample(&image.values, image.size, uv);
            for lane in 0..3 {
                pixel[lane] += glow[lane] / 3.0;
            }
        }
    }
}

pub(super) fn apply(pixels: &mut [[f32; 3]], size: [usize; 2], scene: Scene, style: Style) {
    if style != Style::Textured {
        return;
    }
    if scene.bloom {
        bloom(pixels, size, scene);
    }
    if scene.filmic {
        for pixel in pixels {
            *pixel = pixel.map(film);
        }
    }
}

pub(super) const GLSL: &str = include_str!("output/curve.glsl");
