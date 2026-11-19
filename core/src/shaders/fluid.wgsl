// Screen-space fluid surface: depth smoothing + final shading.
//
// Inputs come from two particle passes (see particles.wgsl):
//   fluid depth : R32Float, eye-space depth of the nearest sphere (1e6 = no water)
//   thickness   : Rgba16Float, additive (thickness, foam * thickness, thickness, 0)

struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    light_dir: vec4<f32>,   // xyz towards light, w = exposure
    viewport: vec4<f32>,    // width, height, time, transparent bg
    bg_color: vec4<f32>,
}

struct Fluid {
    blur: vec4<f32>,    // particle radius (world), pixels per world unit at depth 1, depth falloff (x radius), max radius (px)
    dir: vec4<f32>,     // blur direction (px), _, _
    shade: vec4<f32>,   // refraction, absorption, thickness scale, foam strength
    deep: vec4<f32>,
    shallow: vec4<f32>,
    foam: vec4<f32>,
}

const NONE: f32 = 1e5;

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<uniform> fl: Fluid;
@group(0) @binding(2) var depth_tex: texture_2d<f32>;
@group(0) @binding(3) var thick_tex: texture_2d<f32>;
@group(0) @binding(4) var scene_tex: texture_2d<f32>;
@group(0) @binding(5) var samp: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
}

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> VOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: VOut;
    o.clip = vec4<f32>(uv * 2.0 - vec2<f32>(1.0), 0.0, 1.0);
    return o;
}

// ------------------------------------------------------------ bilateral depth smoothing (separable)
@fragment
fn fs_blur(in: VOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth_tex));
    let px = vec2<i32>(in.clip.xy);
    let z = textureLoad(depth_tex, px, 0).r;
    if (z >= NONE) { return vec4<f32>(z); }

    let r_world = fl.blur.x;
    let radius = clamp(r_world * fl.blur.y / z, 1.0, fl.blur.w);
    let ir = i32(ceil(radius));
    let sigma = max(radius * 0.5, 0.5);
    let zsig = r_world * fl.blur.z;
    let dir = vec2<i32>(fl.dir.xy);
    var sum = 0.0;
    var wsum = 0.0;
    for (var i = -ir; i <= ir; i++) {
        let q = clamp(px + dir * i, vec2<i32>(0), size - vec2<i32>(1));
        var zi = textureLoad(depth_tex, q, 0).r;
        if (zi >= NONE) { continue; }
        let dz = zi - z;
        // narrow-range: samples far in front are clamped so a blob in front cannot pull us forward
        if (dz < -zsig * 3.0) { zi = z - zsig * 3.0; }
        let fi = f32(i);
        let ws = exp(-fi * fi / (2.0 * sigma * sigma));
        let wr = exp(-(zi - z) * (zi - z) / (2.0 * zsig * zsig));
        sum += zi * ws * wr;
        wsum += ws * wr;
    }
    return vec4<f32>(select(z, sum / wsum, wsum > 1e-6));
}

// ------------------------------------------------------------ shading
fn unproject(ndc: vec2<f32>, z: f32) -> vec3<f32> {
    let w = cam.inv_view_proj * vec4<f32>(ndc, z, 1.0);
    return w.xyz / w.w;
}

fn forward() -> vec3<f32> {
    return normalize(cross(cam.cam_up.xyz, cam.cam_right.xyz));
}

fn world_at(px: vec2<i32>, z: f32) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(depth_tex));
    let p = (vec2<f32>(px) + vec2<f32>(0.5)) / size;
    let ndc = vec2<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0);
    let ray = normalize(unproject(ndc, 1.0) - unproject(ndc, 0.0));
    return cam.cam_pos.xyz + ray * (z / max(dot(ray, forward()), 1e-4));
}

fn env(r: vec3<f32>) -> vec3<f32> {
    let horizon = vec3<f32>(0.32, 0.38, 0.46);
    let zenith = vec3<f32>(0.04, 0.10, 0.22);
    let below = vec3<f32>(0.02, 0.022, 0.026);
    let sky = mix(horizon, zenith, smoothstep(0.0, 0.7, r.y));
    return select(mix(below, horizon * 0.6, smoothstep(-0.25, 0.0, r.y)), sky, r.y > 0.0);
}

struct FOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fs_shade(in: VOut) -> FOut {
    let size = vec2<i32>(textureDimensions(depth_tex));
    let px = vec2<i32>(in.clip.xy);
    let z = textureLoad(depth_tex, px, 0).r;
    if (z >= NONE) { discard; }
    let p = world_at(px, z);

    // normal from the smoothed depth, taking the smaller one-sided difference at silhouettes
    let zxp = textureLoad(depth_tex, min(px + vec2<i32>(1, 0), size - vec2<i32>(1)), 0).r;
    let zxm = textureLoad(depth_tex, max(px - vec2<i32>(1, 0), vec2<i32>(0)), 0).r;
    let zyp = textureLoad(depth_tex, min(px + vec2<i32>(0, 1), size - vec2<i32>(1)), 0).r;
    let zym = textureLoad(depth_tex, max(px - vec2<i32>(0, 1), vec2<i32>(0)), 0).r;
    var ddx = world_at(px + vec2<i32>(1, 0), zxp) - p;
    let ddx2 = p - world_at(px - vec2<i32>(1, 0), zxm);
    if (zxp >= NONE || (zxm < NONE && abs(zxm - z) < abs(zxp - z))) { ddx = ddx2; }
    var ddy = world_at(px + vec2<i32>(0, 1), zyp) - p;
    let ddy2 = p - world_at(px - vec2<i32>(0, 1), zym);
    if (zyp >= NONE || (zym < NONE && abs(zym - z) < abs(zyp - z))) { ddy = ddy2; }
    var n = normalize(cross(ddy, ddx));
    let v = normalize(cam.cam_pos.xyz - p);
    if (dot(n, v) < 0.0) { n = -n; }

    let tk = textureLoad(thick_tex, px, 0);
    let thick = tk.x * fl.shade.z;
    let foam = clamp(tk.y / max(tk.z, 1e-4), 0.0, 1.0);
    let t = fl.dir.z;

    // fine wind ripples on surfaces that face up (finer than the particles can resolve)
    let rs = ripple_slope(p.xz * fl.shallow.w, t, 0.04) * fl.deep.w * clamp(n.y * 1.5, 0.0, 1.0);
    n = normalize(n + vec3<f32>(-rs.x, 0.0, -rs.y));

    let exposure = cam.light_dir.w;
    let l = normalize(cam.light_dir.xyz);
    let ndl = max(dot(n, l), 0.0);

    // refraction: bend the background by the surface normal, more where the water is thick
    let uv = (vec2<f32>(px) + vec2<f32>(0.5)) / vec2<f32>(size);
    let bend = vec2<f32>(dot(n, cam.cam_right.xyz), -dot(n, cam.cam_up.xyz));
    let off = bend * fl.shade.x * 0.04 * min(thick * 3.0, 1.0);
    let scene = textureSampleLevel(scene_tex, samp, clamp(uv + off, vec2<f32>(0.001), vec2<f32>(0.999)), 0.0);

    // Beer-Lambert absorption: thin water is clear, deep water takes on the deep colour
    let absorb = (vec3<f32>(1.0) - fl.deep.rgb) * fl.shade.y;
    let trans = exp(-absorb * thick);
    // light scattered back out of the water: bright shallow tint at the edges, deep colour in the body
    let depth_t = 1.0 - exp(-thick * fl.shade.y * 0.6);
    let water_col = mix(fl.shallow.rgb, fl.deep.rgb, depth_t);
    // light glowing through thin sheets and crests (sun behind the water)
    let sss = pow(max(dot(-l, v), 0.0), 3.0) * (1.0 - depth_t) * 0.9;
    let sky_light = sky_color(n, l) * 0.6 + vec3<f32>(0.25) * ndl;
    let inscatter = water_col * (vec3<f32>(1.0) - trans) * (sky_light + vec3<f32>(sss)) * exposure;
    var col = scene.rgb * trans + inscatter;

    // Fresnel reflection of the sky and sun, plus a sharp GGX sun glint
    let fres = 0.02 + 0.98 * pow(1.0 - max(dot(n, v), 0.0), 5.0);
    let r = reflect(-v, n);
    let refl = (sky_color(r, l) + sun_disc(r, l)) * exposure;
    let spec = sun_specular(n, v, l, 0.06) * 2.5;
    col = mix(col, refl, fres) + vec3<f32>(spec * exposure);

    // foam / whitewater: patchy streaks on the surface rather than a white haze
    let tex = foam_texture(p.xz, t);
    let fa = clamp(foam * fl.shade.w, 0.0, 1.0) * smoothstep(0.25, 0.75, tex + foam * 0.35);
    col = mix(col, fl.foam.rgb * (0.5 + 0.5 * ndl) * exposure, fa);

    let t_avg = dot(trans, vec3<f32>(0.3333));
    let alpha = clamp(scene.a * t_avg + (1.0 - t_avg) + fres + fa, 0.0, 1.0);

    var o: FOut;
    o.color = vec4<f32>(col, alpha);
    let clip = cam.view_proj * vec4<f32>(p, 1.0);
    o.depth = clamp(clip.z / clip.w, 0.0, 1.0);
    return o;
}
