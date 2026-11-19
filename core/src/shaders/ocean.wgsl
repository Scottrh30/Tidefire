// Open-ocean surface: a sum of Gerstner waves on a radial grid around the
// camera (dense near, sparse far, out to the horizon). Normals, wave pinching
// (for whitecaps) and fine ripples are evaluated per pixel. The shared sky
// functions (sky.wgsl) are prepended to this source.

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

const NW: u32 = 64u;
// only the longer waves move the mesh; all of them shape the per-pixel normals
const NW_VERTEX: u32 = 28u;

struct Ocean {
    wave_a: array<vec4<f32>, 64>, // direction x, direction z, wavenumber k, amplitude
    wave_b: array<vec4<f32>, 64>, // angular speed, phase, steepness Q, wavelength
    chop: vec4<f32>,  // wind x, wind z, base frequency, amplitude
    chop2: vec4<f32>, // speed
    misc: vec4<f32>,              // time, sea level, ripple strength, ripple scale
    deep: vec4<f32>,              // deep colour, whitecaps amount
    scatter: vec4<f32>,           // crest scatter colour, haze distance (m)
    foam: vec4<f32>,              // foam colour, wave height (for scatter scaling)
    grid: vec4<u32>,              // rings, segments, _, _
    grid_f: vec4<f32>,            // first ring spacing, growth, extent, _
}

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<uniform> O: Ocean;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) base: vec2<f32>,   // undisplaced world xz
    @location(1) world: vec3<f32>,  // displaced position
}

// A wave spanning fewer than ~4 pixels would alias: fade it out by its footprint
// on screen (fp = metres of sea surface covered by one pixel, grazing angle included).
fn wave_fade(i: u32, fp: f32) -> f32 {
    return smoothstep(fp * 3.0, fp * 7.0, O.wave_b[i].w);
}

// The grid gets coarser with distance (spacing ~2.6% of it), so waves shorter
// than ~4 grid cells can't be displaced there without aliasing into streaks;
// they stay in the per-pixel normals instead.
fn vertex_fade(i: u32, dist: f32) -> f32 {
    let wl = O.wave_b[i].w;
    return 1.0 - smoothstep(wl * 6.0, wl * 10.0, dist);
}

// Sharp-crested choppy detail: each octave is a warped grid of peaked
// "1 - |sin|" ridges, which reads like real wind chop far better than smooth sines.
fn chop_octave(p: vec2<f32>, choppy: f32) -> f32 {
    let uv = p + vec2<f32>(vnoise2(p) * 2.0 - 1.0);
    var wv = 1.0 - abs(sin(uv));
    let swv = abs(cos(uv));
    wv = mix(wv, swv, wv);
    return pow(1.0 - pow(wv.x * wv.y, 0.65), choppy);
}

// fine detail height in metres; `fp` = metres per pixel, octaves too small to see fade out
fn chop_height(p_in: vec2<f32>, t: f32, fp: f32) -> f32 {
    let wind = O.chop.xy;
    var freq = O.chop.z;
    var amp = O.chop.w;
    var choppy = 4.0;
    var h = 0.0;
    // rotate each octave so the pattern never lines up
    let m = mat2x2<f32>(1.6, 1.2, -1.2, 1.6);
    var p = vec2<f32>(p_in.x * wind.x + p_in.y * wind.y, -p_in.x * wind.y + p_in.y * wind.x);
    p.y *= 0.45; // ridges stretched along the crests
    for (var i = 0; i < 5; i++) {
        let wl = 6.2832 / freq;
        let f = smoothstep(fp * 6.0, fp * 16.0, wl);
        // phase speed of a deep-water wave of this length, in metres per second
        let off = vec2<f32>(t * sqrt(9.81 / freq) * O.chop2.x, 0.0);
        var d = chop_octave((p + off) * freq, choppy);
        d += chop_octave((p - off * 0.7) * freq, choppy);
        h += d * amp * f;
        p = m * p;
        freq *= 1.9;
        amp *= 0.4;
        choppy = mix(choppy, 1.0, 0.2);
    }
    return h;
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    let rings = O.grid.x;
    let segs = O.grid.y;
    let ring = vi / segs;
    let seg = vi % segs;
    // ring radius grows geometrically: fine near the camera, coarse at the horizon
    let g = O.grid_f.y;
    let r = O.grid_f.x * (pow(g, f32(ring)) - 1.0) / (g - 1.0);
    let ang = f32(seg) / f32(segs) * 6.2831853;
    let centre = cam.cam_pos.xz;
    let base = centre + vec2<f32>(cos(ang), sin(ang)) * r;
    let t = O.misc.x;
    var p = vec3<f32>(base.x, O.misc.y, base.y);
    for (var i = 0u; i < NW_VERTEX; i++) {
        let a = O.wave_a[i];
        let b = O.wave_b[i];
        let f = vertex_fade(i, r);
        let th = a.z * dot(a.xy, base) - b.x * t + b.y;
        p.x -= b.z * a.w * a.x * sin(th) * f;
        p.z -= b.z * a.w * a.y * sin(th) * f;
        p.y += a.w * cos(th) * f;
    }
    var o: VOut;
    o.clip = cam.view_proj * vec4<f32>(p, 1.0);
    o.base = base;
    o.world = p;
    return o;
}

struct FOut {
    @location(0) color: vec4<f32>,
}

@fragment
fn fs_main(in: VOut) -> FOut {
    let t = O.misc.x;
    let dist = length(in.world.xz - cam.cam_pos.xz);
    // pixel footprint on the sea; the geometric mean keeps waves crossing the view
    // visible far out, where the footprint is long and thin
    let fp = sqrt(length(dpdx(in.base)) * length(dpdy(in.base)));
    // analytic Gerstner derivatives (tangents) and the Jacobian of the horizontal squeeze
    var dxx = 1.0;
    var dzz = 1.0;
    var dxz = 0.0;
    var dyx = 0.0;
    var dyz = 0.0;
    var height = 0.0;
    // whitecaps use only waves several pixels long, or they sparkle as single-pixel dots
    var cxx = 1.0;
    var czz = 1.0;
    var cxz = 0.0;
    var cyx = 0.0;
    var cyz = 0.0;
    for (var i = 0u; i < NW; i++) {
        let a = O.wave_a[i];
        let b = O.wave_b[i];
        let f = wave_fade(i, fp);
        let th = a.z * dot(a.xy, in.base) - b.x * t + b.y;
        let s = sin(th);
        let c = cos(th);
        let wa = a.z * a.w * f;
        dxx -= b.z * wa * a.x * a.x * c;
        dzz -= b.z * wa * a.y * a.y * c;
        dxz -= b.z * wa * a.x * a.y * c;
        let g = smoothstep(fp * 12.0, fp * 30.0, b.w) * b.z * a.z * a.w * c;
        cxx -= g * a.x * a.x;
        czz -= g * a.y * a.y;
        cxz -= g * a.x * a.y;
        let gs = smoothstep(fp * 12.0, fp * 30.0, b.w) * a.z * a.w * s;
        cyx -= gs * a.x;
        cyz -= gs * a.y;
        dyx -= wa * a.x * s;
        dyz -= wa * a.y * s;
        height += a.w * c * f;
    }
    let tx = vec3<f32>(dxx, dyx, dxz);
    let tz = vec3<f32>(dxz, dyz, dzz);
    var n = normalize(cross(tz, tx));
    let jac = cxx * czz - cxz * cxz;
    // a smoother normal (only waves many pixels long) for the Fresnel term: Fresnel is so
    // non-linear at grazing angles that pixel-sized slopes turn into sparkling dots
    let n_soft = normalize(cross(vec3<f32>(cxz, cyz, czz), vec3<f32>(cxx, cyx, cxz)));

    // sharp wind chop on top (finite-difference slope of the detail height)
    if (O.chop.w > 0.0) {
        let e = max(fp * 2.5, 0.01); // wider than a pixel: the cusps would alias
        let h0 = chop_height(in.base, t, fp);
        let hx = chop_height(in.base + vec2<f32>(e, 0.0), t, fp);
        let hz = chop_height(in.base + vec2<f32>(0.0, e), t, fp);
        var cs = vec2<f32>(hx - h0, hz - h0) / e;
        // the ridges are cusps: cap the slope so single pixels don't flip to face the sky
        cs = cs / max(1.0, length(cs) / 0.3);
        n = normalize(n + vec3<f32>(-cs.x, 0.0, -cs.y));
        height += (h0 - O.chop.w) * 0.5;
    }
    // fine wind ripples on top, fading with distance
    // ripples must fade before they get smaller than a pixel, or they shimmer
    let rfade = smoothstep(fp * 3.0, fp * 7.0, 0.25 / max(O.misc.w, 0.1));
    let rs = ripple_slope(in.base * O.misc.w, t * 1.3, 0.05) * O.misc.z * rfade;
    n = normalize(n + vec3<f32>(-rs.x, 0.0, -rs.y));

    let v = normalize(cam.cam_pos.xyz - in.world);
    let l = normalize(cam.light_dir.xyz);
    let exposure = cam.light_dir.w;
    // faces turned away from the camera (hidden behind crests) would mirror the sky as bright seams
    // A face turned away from the camera is really hidden behind its crest; what the
    // pixel sees is the crest's front, which faces the camera. Mirror it for the Fresnel.
    let raw = dot(n, v);
    if (raw < 0.08) { n = normalize(n + v * (0.08 - raw)); }
    let ndv = select(max(dot(n, v), 0.0), min(0.16 - raw, 1.0), raw < 0.08);
    let ndl = max(dot(n, l), 0.0);

    // reflection of the sky and sun
    let ndv_soft = clamp(abs(dot(n_soft, v)), 0.0, 1.0);
    let fres = 0.02 + 0.98 * pow(1.0 - mix(ndv_soft, ndv, 0.3), 5.0);
    let r = reflect(-v, n);
    // (the sun itself is left to the specular term: its tiny, very bright disc
    // reflected in pixel-sized facets sparkles like noise)
    let refl = sky_base(r) * vec3<f32>(1.1, 1.25, 1.3);
    // specular anti-aliasing: widen the highlight where the normal changes fast between
    // pixels, so the sun spreads into a soft sheen instead of single-pixel glints
    let nvar = length(fwidth(n));
    let spec = sun_specular(n, v, l, clamp(0.3 + nvar * 1.5 + dist * 0.0004, 0.0, 1.0));

    // light coming out of the water: dark deep body, teal glow through crests
    // the water body is lit mostly by the whole sky, a little by the sun
    let body = O.deep.rgb * (0.12 + 0.88 * ndl * ndl) * (sky_color(vec3<f32>(0.0, 1.0, 0.0), l) * 1.3 + vec3<f32>(0.1));
    let crest = clamp(height / max(O.foam.w, 0.01) * 0.6 + 0.3, 0.0, 1.0);
    let back = pow(max(dot(v, -l) * 0.5 + 0.5, 0.0), 3.0);
    let thin = pow(1.0 - ndv, 2.0);
    let sss = O.scatter.rgb * crest * crest * (0.25 + 1.2 * back) * (0.4 + thin);
    var col = mix(body + sss, refl, fres) + vec3<f32>(spec * 0.004);

    // whitecaps where the waves pinch together, broken into streaky patches
    let pinch = clamp((1.0 - jac) * 1.6, 0.0, 1.0);
    let caps = smoothstep(1.0 - O.deep.w, 1.0, pinch) * smoothstep(0.45, 0.8, foam_texture(in.base * 2.2, t));
    col = mix(col, O.foam.rgb * (0.45 + 0.55 * ndl) * (sky_color(vec3<f32>(0.0, 1.0, 0.0), l) * 1.3 + vec3<f32>(0.2)), clamp(caps, 0.0, 1.0));

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
