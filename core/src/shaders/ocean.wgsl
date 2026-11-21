// Open-ocean surface: FFT ocean cascades (ocean_fft.wgsl) sampled on a radial
// grid around the camera (dense near, sparse far, out to the horizon). The grid
// is displaced by the cascades' mip levels that suit its spacing; normals,
// folding (Jacobian) and foam are sampled per pixel with mipmapped, anisotropic
// filtering. The shared sky functions (sky.wgsl) are prepended to this source.

struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    light_dir: vec4<f32>,   // towards the sun, w = exposure
    viewport: vec4<f32>,
    bg_color: vec4<f32>,
}

struct Ocean {
    cascade: vec4<f32>,  // tile sizes of the 3 cascades (m), choppiness lambda
    misc: vec4<f32>,     // time, sea level, foam threshold (Jacobian), _
    deep: vec4<f32>,     // deep colour, whitecaps amount
    scatter: vec4<f32>,  // crest scatter colour, haze distance (m)
    foam: vec4<f32>,     // foam colour, significant wave height (m)
    grid: vec4<u32>,     // rings, segments, _, _
    grid_f: vec4<f32>,   // first ring spacing, growth, extent, metres per pixel at 1 m
    light: vec4<f32>,    // scattering strength, absorption scale, shadow strength, sun intensity
}

@group(0) @binding(1) var<uniform> O: Ocean;
@group(0) @binding(2) var disp_tex: texture_2d_array<f32>;   // dx, dy, dz, dDx/dz
@group(0) @binding(3) var deriv_tex: texture_2d_array<f32>;  // dh/dx, dh/dz, dDx/dx, dDz/dz
@group(0) @binding(4) var foam_tex: texture_2d_array<f32>;   // foam, jacobian
@group(0) @binding(5) var wave_samp: sampler;

const TEXELS: f32 = 256.0;

@group(0) @binding(0) var<uniform> cam: Camera;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) base: vec2<f32>,   // undisplaced world xz (where the textures are sampled)
    @location(1) world: vec3<f32>,  // displaced position
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    let segs = O.grid.y;
    let ring = vi / segs;
    let seg = vi % segs;
    // ring radius grows geometrically: fine near the camera, coarse at the horizon
    let g = O.grid_f.y;
    let r = O.grid_f.x * (pow(g, f32(ring)) - 1.0) / (g - 1.0);
    let ang = f32(seg) / f32(segs) * 6.2831853;
    let base = cam.cam_pos.xz + vec2<f32>(cos(ang), sin(ang)) * r;
    // grid spacing here; waves shorter than a couple of cells are left to the
    // per-pixel normals by reading a blurrier mip level
    let spacing = O.grid_f.x * pow(g, f32(ring)) + r * 0.0123;
    var d = vec3<f32>(0.0);
    for (var c = 0; c < 3; c++) {
        let L = O.cascade[c];
        let lod = log2(max(spacing * 2.0 / (L / TEXELS), 1.0));
        d += textureSampleLevel(disp_tex, wave_samp, base / L, c, lod).xyz;
    }
    let p = vec3<f32>(base.x + d.x, O.misc.y + d.y, base.y + d.z);
    var o: VOut;
    o.clip = cam.view_proj * vec4<f32>(p, 1.0);
    o.base = base;
    o.world = p;
    return o;
}

struct FOut {
    @location(0) color: vec4<f32>,
}

// ---- light inside the water

// Height of the sea surface above world point xz, from the two big cascades at a
// blurred mip (the light paths don't need ripples). The waves are displaced
// sideways too: one fixed-point step finds the undisplaced point under xz.
fn surface_y(xz: vec2<f32>) -> f32 {
    var d = vec3<f32>(0.0);
    for (var c = 0; c < 2; c++) {
        d += textureSampleLevel(disp_tex, wave_samp, xz / O.cascade[c], c, 2.0).xyz;
    }
    var h = 0.0;
    for (var c = 0; c < 2; c++) {
        h += textureSampleLevel(disp_tex, wave_samp, (xz - d.xz) / O.cascade[c], c, 2.0).y;
    }
    return O.misc.y + h;
}

// Distance sunlight travels through water to reach underwater point q, going
// back along the refracted sun direction `lu` (pointing up) until it leaves
// the wave. Near a thin crest that is short, so the crest glows; deep inside
// a wave or a trough it is long, so the light is absorbed.
fn light_path(q: vec3<f32>, lu: vec3<f32>, step: f32) -> f32 {
    var t_prev = 0.0;
    var under_prev = surface_y(q.xz) - q.y;
    if (under_prev <= 0.0) { return 0.0; }
    for (var j = 1; j <= 5; j++) {
        let t = step * f32(j * j);
        let p = q + lu * t;
        let under = surface_y(p.xz) - p.y;
        if (under <= 0.0) {
            // crossed the surface between the two samples
            return mix(t_prev, t, under_prev / max(under_prev - under, 1e-4));
        }
        t_prev = t;
        under_prev = under;
    }
    // still inside after the last step: finish straight up to the surface
    return t_prev + under_prev / max(lu.y, 0.1);
}

fn henyey_greenstein(cos_t: f32, g: f32) -> f32 {
    let d = 1.0 + g * g - 2.0 * g * cos_t;
    return (1.0 - g * g) / (12.566 * pow(max(d, 1e-4), 1.5));
}

struct Underwater {
    sun: vec3<f32>,        // sunlight scattered towards the camera
    transmit: vec3<f32>,   // how much of the light from behind gets through
}

// Sunlight scattered once inside the water towards the camera (single scattering
// along the refracted view ray), with Beer-Lambert absorption per colour.
// Absorption follows sea water (red goes first). Where the view ray passes out
// through the back of a thin crest the march stops: such water holds only a short,
// sunlit path, so it glows bright teal, while open water, seen deep, turns blue.
fn sun_scatter(p: vec3<f32>, rv: vec3<f32>, lu: vec3<f32>, wave_len: f32, near: bool) -> Underwater {
    // sea water with a little plankton (coefficients per metre)
    let sigma_a = vec3<f32>(0.6, 0.12, 0.07) * O.light.y;
    let sigma_s = 0.15 * O.light.x;
    let sigma_t = sigma_a + vec3<f32>(sigma_s);
    let phase = mix(0.08, henyey_greenstein(dot(lu, rv), 0.6), 0.8);
    let step = max(wave_len * 0.02, 0.03);
    var acc = vec3<f32>(0.0);
    var s0 = 0.0;
    var out: Underwater;
    out.transmit = vec3<f32>(0.0);
    for (var i = 1; i <= 8; i++) {
        let s1 = 0.19 * f32(i * i); // 0.19 .. 12 m along the ray
        let s = 0.5 * (s0 + s1);
        let q = p + rv * s;
        // (only near the camera: far off the blurred heights are too coarse to tell)
        if (near && i > 1 && surface_y(q.xz) < q.y) {
            // the ray has left the water through the back of a crest
            out.transmit = exp(-sigma_t * s0);
            break;
        }
        let dl = light_path(q, lu, step);
        acc += exp(-sigma_t * (s + dl)) * (s1 - s0);
        s0 = s1;
    }
    out.sun = acc * sigma_s * phase;
    return out;
}

// How much of the reflected ray from p reaches the sky (1) rather than running
// into a nearby wave (0). Marched over the smooth big-wave heights, with a soft
// edge so the result doesn't flicker as the waves move.
fn sky_visibility(p: vec3<f32>, r: vec3<f32>, wave_len: f32) -> f32 {
    var vis = 1.0;
    let step = max(wave_len * 0.04, 0.08);
    for (var j = 1; j <= 7; j++) {
        let t = step * f32(j * j);
        let q = p + r * t;
        let above = q.y - surface_y(q.xz);
        vis = min(vis, clamp(above / (t * 0.25) + 0.5, 0.0, 1.0));
    }
    return vis;
}

// Soft shadow from other waves on point p, marching towards the sun above water.
fn sun_shadow(p: vec3<f32>, l: vec3<f32>, wave_len: f32) -> f32 {
    var lit = 1.0;
    let step = max(wave_len * 0.05, 0.1);
    for (var j = 1; j <= 6; j++) {
        let t = step * f32(j * j);
        let q = p + l * t;
        let above = q.y - surface_y(q.xz);
        lit = min(lit, clamp(above / (t * 0.4) + 0.5, 0.0, 1.0));
    }
    return lit;
}

@fragment
fn fs_main(in: VOut) -> FOut {
    let t = O.misc.x;
    let dist = length(in.world.xz - cam.cam_pos.xz);
    // sum the cascades: slopes, Jacobian terms, height and foam
    var sl = vec2<f32>(0.0);
    var sl_soft = vec2<f32>(0.0);
    var dxx = 0.0;
    var dzz = 0.0;
    var dxz = 0.0;
    var height = 0.0;
    var foam = 0.0;
    let foam_w = vec3<f32>(1.0, 0.6, 0.15);
    for (var c = 0; c < 3; c++) {
        let uv = in.base / O.cascade[c];
        let dv = textureSample(deriv_tex, wave_samp, uv, c);
        let ds = textureSample(disp_tex, wave_samp, uv, c);
        // a blurrier read for the Fresnel term: at grazing angles Fresnel is so
        // non-linear that pixel-sized slopes would sparkle
        let dsoft = textureSampleBias(deriv_tex, wave_samp, uv, c, 1.0);
        let fo = textureSample(foam_tex, wave_samp, uv, c).x;
        sl += dv.xy;
        sl_soft += dsoft.xy;
        dxx += dv.z;
        dzz += dv.w;
        dxz += ds.w;
        height += ds.y;
        foam += fo * foam_w[c];
    }
    // slopes are per undisplaced metre; the choppy squeeze makes them steeper
    let sx = sl.x / max(1.0 + dxx, 0.25);
    let sz = sl.y / max(1.0 + dzz, 0.25);
    var n = normalize(vec3<f32>(-sx, 1.0, -sz));
    let n_soft = normalize(vec3<f32>(-sl_soft.x, 1.0, -sl_soft.y));
    let jac = (1.0 + dxx) * (1.0 + dzz) - dxz * dxz;

    let v = normalize(cam.cam_pos.xyz - in.world);
    let l = normalize(cam.light_dir.xyz);
    let exposure = cam.light_dir.w;
    // A face turned away from the camera is really hidden behind its crest; what the
    // pixel sees is the crest's front, which faces the camera. Mirror it for the Fresnel.
    let raw = dot(n, v);
    if (raw < 0.08) { n = normalize(n + v * (0.08 - raw)); }
    let ndv = select(max(dot(n, v), 0.0), min(0.16 - raw, 1.0), raw < 0.08);
    let ndl = max(dot(n, l), 0.0);

    // reflection of the sky
    let ndv_soft = clamp(abs(dot(n_soft, v)), 0.0, 1.0);
    let fres = 0.02 + 0.98 * pow(1.0 - mix(ndv_soft, ndv, 0.8), 5.0);
    let r = reflect(-v, n);
    // a ray reflected downwards would hit the next wave, not the ground below the
    // horizon: blend from the horizon sky into dark sea
    let sky_r = sky_base(vec3<f32>(r.x, max(r.y, 0.01), r.z)) * vec3<f32>(1.25, 1.42, 1.5);
    let sea_r = O.deep.rgb * (sky_color(vec3<f32>(0.0, 1.0, 0.0), l) * 1.3 + vec3<f32>(0.1)) * 1.5;
    var refl = mix(sky_r, sea_r, smoothstep(0.0, 0.25, -r.y));
    // specular anti-aliasing: widen the highlight where the normal changes fast
    let nvar = length(fwidth(n));
    let spec = sun_specular(n, v, l, clamp(0.25 + nvar * 1.5 + dist * 0.0004, 0.0, 1.0));

    // light coming out of the water: skylight scattered back from the depths
    // (the deep colour), plus sunlight scattered inside the waves
    let sky_up = sky_color(vec3<f32>(0.0, 1.0, 0.0), l);
    let wave_len = O.cascade[0] / 10.0;
    // lit by the sun unless another wave is in the way (near the camera only:
    // far away a shadow would be smaller than a pixel)
    // The light paths run against the smooth big-wave surface (surface_y), so
    // start them on it: the detailed surface differs from it by the small waves.
    let p_smooth = vec3<f32>(in.world.x, surface_y(in.world.xz), in.world.z);
    // Reflection occlusion: in a trough the reflected ray often hits the next wave,
    // which shows its dark water instead of bright sky. (Near the camera only: far
    // away the blurred heights are too coarse, and the haze takes over.)
    var sky_vis = 1.0;
    if (O.light.z > 0.0 && dist < wave_len * 12.0) {
        let vis = sky_visibility(p_smooth + vec3<f32>(0.0, 0.05, 0.0), r, wave_len);
        sky_vis = mix(1.0, vis, O.light.z * (1.0 - smoothstep(wave_len * 6.0, wave_len * 12.0, dist)));
        refl = mix(sea_r * 0.7, refl, sky_vis);
    }
    var shadow = 1.0;
    if (O.light.z > 0.0 && dist < wave_len * 8.0) {
        shadow = mix(1.0, sun_shadow(p_smooth, l, wave_len), O.light.z * (1.0 - smoothstep(wave_len * 4.0, wave_len * 8.0, dist)));
    }
    let ambient = O.deep.rgb * (sky_up * 1.3 + vec3<f32>(0.1)) * (0.75 + 0.25 * shadow);
    // troughs see less sky than crests, so less skylight scatters back out of them
    let crest_k = clamp(height / max(O.foam.w, 0.01) + 0.5, 0.0, 1.0);
    let ambient_k = ambient * mix(0.45, 1.15, crest_k);
    var body = ambient_k;
    var sss = vec3<f32>(0.0);
    if (O.light.x > 0.0 && dist < wave_len * 60.0) {
        let rv = refract(-v, n, 0.752);
        // Sunlight entering a flat sea bends towards the vertical, but most of the
        // light that lights up a wave goes in through its steep sun-facing back,
        // nearly head-on, and keeps its low angle through the crest.
        let l_flat = vec3<f32>(l.x * 0.752, sqrt(max(1.0 - (1.0 - l.y * l.y) * 0.565, 0.0)), l.z * 0.752);
        let lu = normalize(mix(l_flat, l, 0.75));
        let sun = vec3<f32>(1.0, 0.95, 0.88) * O.light.w;
        let uw = sun_scatter(p_smooth, rv, lu, wave_len, dist < wave_len * 10.0);
        let tint = O.scatter.rgb / max(dot(O.scatter.rgb, vec3<f32>(0.333)), 1e-3);
        let fade = 1.0 - smoothstep(wave_len * 30.0, wave_len * 60.0, dist);
        // a wave in the way blocks the sunlight before it enters the water: on water
        // a shadow shows as missing glow from below (and on foam), not as a dark patch
        sss = sun * uw.sun * tint * fade * mix(1.0, shadow, 0.85);
        // through a thin crest the camera sees the sunlit water and sky behind it
        let behind = (sky_color(vec3<f32>(rv.x, 0.15, rv.z), l) * 0.5 + ambient) * uw.transmit * tint;
        body = mix(ambient_k, ambient_k * 0.6 + behind, fade);
    }
    var col = mix(body + sss, refl, fres) + vec3<f32>(spec * 0.01 * shadow);

    // whitecaps: fresh foam where the waves fold now, older foam fading in streaks
    let fresh = smoothstep(0.0, 0.3, O.misc.z - jac);
    let streaks = smoothstep(0.15, 0.8, foam) * mix(0.4, 1.0, foam_texture(in.base * 1.5, t));
    let caps = clamp(max(fresh, streaks) * smoothstep(0.0, 0.05, O.deep.w), 0.0, 1.0);
    // foam is a bright diffuse surface, lit by the whole sky and the sun
    let foam_col = O.foam.rgb * (0.55 + 0.25 * max(n.y, 0.0) + 0.3 * ndl * shadow);
    col = mix(col, foam_col, caps);

    // aerial haze towards the horizon
    let haze = 1.0 - exp(-dist / max(O.scatter.w, 1.0));
    let horizon = sky_color(vec3<f32>(-v.x, 0.02, -v.z), l);
    col = mix(col, horizon, haze);

    // fade out at the edge of the grid
    let edge = 1.0 - smoothstep(O.grid_f.z * 0.75, O.grid_f.z, dist);
    col = mix(horizon, col, edge);
    var o: FOut;
    o.color = vec4<f32>(col * exposure, 1.0);
    return o;
}

// ---- sky background (drawn behind everything when the sky is switched on)
struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) vi: u32) -> SkyOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: SkyOut;
    o.clip = vec4<f32>(uv * 2.0 - vec2<f32>(1.0), 1.0, 1.0);
    o.ndc = uv * 2.0 - vec2<f32>(1.0);
    return o;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let a = cam.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let b = cam.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    var dir = normalize(b.xyz / b.w - a.xyz / a.w);
    // below the horizon (past the end of the sea) show the hazy horizon colour, not a dark band
    dir.y = max(dir.y, 0.003);
    let l = normalize(cam.light_dir.xyz);
    return vec4<f32>((sky_color(dir, l) + sun_disc(dir, l) * 0.5) * cam.light_dir.w, 1.0);
}
