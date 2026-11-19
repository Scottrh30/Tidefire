// Copies a finished frame onto the window/canvas surface.

struct Present {
    flags: vec4<f32>, // x: encode sRGB by hand (surface is not sRGB), y: checkerboard behind transparency
}

@group(0) @binding(0) var<uniform> P: Present;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: VOut;
    o.clip = vec4<f32>(uv * 2.0 - vec2<f32>(1.0), 0.0, 1.0);
    o.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return o;
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(src, samp, in.uv, 0.0); // premultiplied, linear
    var rgb = c.rgb;
    if (P.flags.y > 0.5) {
        let cell = vec2<i32>(floor(in.clip.xy / 12.0));
        let k = select(0.018, 0.035, ((cell.x + cell.y) & 1) == 0);
        rgb = rgb + vec3<f32>(k) * (1.0 - c.a);
    }
    if (P.flags.x > 0.5) { rgb = to_srgb(rgb); }
    return vec4<f32>(rgb, 1.0);
}
