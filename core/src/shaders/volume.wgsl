// Raymarched fire + smoke volume, composited over the opaque scene (premultiplied alpha)

struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    light_dir: vec4<f32>,
    viewport: vec4<f32>,
    bg_color: vec4<f32>,
}

struct Vol {
    box_min: vec4<f32>,     // xyz, step scale
    box_max: vec4<f32>,     // xyz, max steps
    smoke_color: vec4<f32>, // rgb, absorption
    fire: vec4<f32>,        // flame intensity, temperature scale, smoke self-illumination, shadow density
    ambient: vec4<f32>,     // rgb ambient, fuel glow
    detail: vec4<f32>,      // flame detail amount, detail scale, detail rise speed, sim time
}

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<uniform> vol: Vol;
@group(0) @binding(2) var tex: texture_3d<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var depth_tex: texture_depth_2d;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    let p = uv * 2.0 - vec2<f32>(1.0);
    var o: VOut;
    o.clip = vec4<f32>(p, 0.0, 1.0);
    o.ndc = p;
    return o;
}

fn unproject(ndc: vec2<f32>, z: f32) -> vec3<f32> {
    let w = cam.inv_view_proj * vec4<f32>(ndc, z, 1.0);
    return w.xyz / w.w;
}

// Stylised blackbody ramp: deep red -> orange -> yellow -> white-blue core
fn fire_ramp(t: f32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0);
    let c1 = vec3<f32>(0.35, 0.02, 0.0);
    let c2 = vec3<f32>(1.0, 0.11, 0.0);
    let c3 = vec3<f32>(1.0, 0.17, 0.01);
    let c4 = vec3<f32>(1.0, 0.27, 0.035);
    if (x < 0.33) { return mix(vec3<f32>(0.0), c1, x / 0.33); }
    if (x < 0.6) { return mix(c1, c2, (x - 0.33) / 0.27); }
    if (x < 0.85) { return mix(c2, c3, (x - 0.6) / 0.25); }
    return mix(c3, c4, (x - 0.85) / 0.15);
}

// ---- value noise, for flame detail finer than the simulation grid
fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * 0.3183099 + vec3<f32>(0.1, 0.17, 0.13)) * 17.0;
    return fract(q.x * q.y * q.z * (q.x + q.y + q.z));
}

fn vnoise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x),
                mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x),
                mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

// Breaks hot gas into licking tongues and thin bright sheets: noise stretched
// vertically and rushing upward, domain-warped so it curls, with ridges
// (bright edges) and gaps (dark between tongues). 1 where detail is off.
fn flame_detail(p: vec3<f32>, tt: f32) -> f32 {
    let amount = vol.detail.x;
    if (amount <= 0.0) { return 1.0; }
    let rise = vol.detail.w * vol.detail.z;
    let q = (p - vec3<f32>(0.0, rise, 0.0)) * vol.detail.y * vec3<f32>(1.0, 0.42, 1.0);
    let warp = vec3<f32>(vnoise(q * 0.55 + vec3<f32>(3.1, 0.0, 0.0)), vnoise(q * 0.55 + vec3<f32>(0.0, 7.7, 0.0)), vnoise(q * 0.55 + vec3<f32>(0.0, 0.0, 5.3))) - vec3<f32>(0.5);
    let w = q + warp * 2.2;
    let n = 0.6 * vnoise(w) + 0.3 * vnoise(w * 2.07 + vec3<f32>(1.3)) + 0.1 * vnoise(w * 4.1 + vec3<f32>(4.7));
    let ridge = 1.0 - abs(2.0 * n - 1.0);
    // the hottest gas survives; cooler edges are carved into tongues with gaps
    // a slow, large-scale layer opens big gaps between separate tongues
    let big = vnoise(q * vec3<f32>(0.16, 0.22, 0.16) + warp * 0.6 + vec3<f32>(11.0, 0.0, 3.0));
    let carve = smoothstep(0.32, 0.68, n + (tt - 0.5) * 0.8 + (big - 0.5) * 0.9);
    let r2 = ridge * ridge;
    let r8 = r2 * r2 * r2 * r2;
    // thin bright sheets (ridges) over a dimmer body, like flame fronts
    let d = carve * (0.22 + 0.9 * r2 * r2 + 1.8 * r8);
    return mix(1.0, d * 1.25, amount);
}

// Each channel saturates on its own, so bright fire goes yellow (as on camera)
// rather than flat white.
fn film(x: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(1.0) - exp(-x);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51; let b = 0.03; let c = 2.43; let d = 0.59; let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn sample_vol(p: vec3<f32>) -> vec4<f32> {
    let uvw = (p - vol.box_min.xyz) / (vol.box_max.xyz - vol.box_min.xyz);
    // soften the domain walls so smoke leaving the box never shows a hard edge
    let e = min(uvw, vec3<f32>(1.0) - uvw);
    let fade = smoothstep(0.0, 0.04, e.x) * smoothstep(0.0, 0.10, 1.0 - uvw.y) * smoothstep(0.0, 0.04, e.z);
    return textureSampleLevel(tex, samp, uvw, 0.0) * fade;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.clip.xy);
    let d = textureLoad(depth_tex, px, 0);
    let near = unproject(in.ndc, 0.0);
    let far = unproject(in.ndc, 1.0);
    let ro = cam.cam_pos.xyz;
    let rd = normalize(far - near);
    let scene_pt = unproject(in.ndc, d);
    let scene_t = select(1e9, length(scene_pt - ro), d < 1.0);

    // ray / box
    let bmin = vol.box_min.xyz;
    let bmax = vol.box_max.xyz;
    let inv = 1.0 / rd;
    let t0 = (bmin - ro) * inv;
    let t1 = (bmax - ro) * inv;
    let tmin3 = min(t0, t1);
    let tmax3 = max(t0, t1);
    var tn = max(max(tmin3.x, tmin3.y), max(tmin3.z, 0.0));
    let tf = min(min(min(tmax3.x, tmax3.y), tmax3.z), scene_t);
    if (tf <= tn) { discard; }

    let extent = bmax - bmin;
    let max_steps = i32(vol.box_max.w);
    let step = max(length(extent) / f32(max_steps), 1e-4) * vol.box_min.w;
    // jitter start to hide banding
    let jitter = fract(sin(dot(in.clip.xy + vec2<f32>(cam.viewport.z * 13.1), vec2<f32>(12.9898, 78.233))) * 43758.5453);
    var t = tn + step * jitter;

    let sigma = vol.smoke_color.w;
    let ldir = normalize(cam.light_dir.xyz);
    let lstep = length(extent) / 24.0;
    var trans = 1.0;
    var col = vec3<f32>(0.0);
    var emis_alpha = 0.0;

    for (var i = 0; i < 512; i++) {
        if (i >= max_steps || t > tf || trans < 0.01) { break; }
        let p = ro + rd * t;
        let s = sample_vol(p);
        let smoke = s.x;
        let temp = s.y;
        let fuel = s.z;
        let react = s.w;
        // emission from hot gas
        let tt = temp * vol.fire.y;
        var flame = fire_ramp(tt) * pow(tt, 1.6) * vol.fire.x + vec3<f32>(1.0, 0.45, 0.1) * react * 0.02 * vol.fire.x;
        if (tt > 0.02) { flame *= flame_detail(p, tt); }
        flame += vec3<f32>(0.2, 0.4, 1.0) * fuel * vol.ambient.w;
        let dens = smoke * sigma;
        if (dens > 1e-4 || dot(flame, vec3<f32>(1.0)) > 1e-4) {
            // light visibility (short shadow march)
            var od = 0.0;
            if (dens > 1e-3) {
                for (var j = 1; j <= 6; j++) {
                    let q = p + ldir * lstep * f32(j);
                    od += sample_vol(q).x;
                }
            }
            let shadow = exp(-od * sigma * lstep * vol.fire.w);
            let lit = vol.smoke_color.rgb * (vol.ambient.rgb + vec3<f32>(1.0, 0.97, 0.92) * shadow)
                    + fire_ramp(tt) * min(tt, 2.0) * vol.fire.z;
            let a = 1.0 - exp(-dens * step);
            col += trans * (lit * a + flame * step);
            emis_alpha += trans * dot(flame * step, vec3<f32>(0.333));
            trans *= 1.0 - a;
        }
        t += step;
    }
    let exposure = cam.light_dir.w;
    let c = film(col * exposure);
    let alpha = clamp(1.0 - trans + emis_alpha * exposure, 0.0, 1.0);
    return vec4<f32>(c, alpha);
}
