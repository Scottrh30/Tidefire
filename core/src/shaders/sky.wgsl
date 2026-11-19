// Shared sky / sun model (prepended to the water, ocean and scene shaders).
// A clear daytime sky: deep blue overhead, pale and hazy at the horizon, a
// darker band below it (what reflections of the ground/sea look like), plus a
// sun disc with a soft glow. Linear colour; exposure is applied by the caller.

// sky without the warm glow around the sun
fn sky_base(dir_in: vec3<f32>) -> vec3<f32> {
    let dir = normalize(dir_in);
    let up = dir.y;
    let zenith = vec3<f32>(0.07, 0.22, 0.52);
    let horizon = vec3<f32>(0.45, 0.62, 0.8);
    let below = vec3<f32>(0.10, 0.13, 0.17);
    var c: vec3<f32>;
    if (up >= 0.0) {
        c = mix(horizon, zenith, pow(smoothstep(0.0, 1.0, up), 0.55));
    } else {
        c = mix(horizon * 0.7, below, smoothstep(0.0, 0.25, -up));
    }
    return c;
}

fn sky_color(dir: vec3<f32>, sun: vec3<f32>) -> vec3<f32> {
    let s = max(dot(normalize(dir), normalize(sun)), 0.0);
    return sky_base(dir) + vec3<f32>(1.0, 0.85, 0.65) * (pow(s, 8.0) * 0.18 + pow(s, 64.0) * 0.5);
}

// The sun's own disc, for glints in reflections.
fn sun_disc(dir: vec3<f32>, sun: vec3<f32>) -> vec3<f32> {
    let s = max(dot(normalize(dir), normalize(sun)), 0.0);
    return vec3<f32>(1.0, 0.92, 0.8) * smoothstep(0.9993, 0.9998, s) * 40.0;
}

// GGX specular for the sun (roughness 0..1), already multiplied by N.L.
fn sun_specular(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, rough: f32) -> f32 {
    let h = normalize(v + l);
    let nh = max(dot(n, h), 0.0);
    let nl = max(dot(n, l), 0.0);
    let a = rough * rough;
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return nl * a2 / (3.14159 * d * d + 1e-5) * 0.25;
}

// ---- gradient-ish noise for ripples and foam texture
fn sky_hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn vnoise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = sky_hash2(i);
    let b = sky_hash2(i + vec2<f32>(1.0, 0.0));
    let c = sky_hash2(i + vec2<f32>(0.0, 1.0));
    let d = sky_hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Height of small wind ripples at world xz (several drifting octaves).
fn ripple_height(p: vec2<f32>, t: f32) -> f32 {
    var h = 0.0;
    var amp = 1.0;
    var q = p;
    let dirs = array<vec2<f32>, 4>(vec2<f32>(0.8, 0.6), vec2<f32>(-0.5, 0.86), vec2<f32>(0.3, -0.95), vec2<f32>(-0.9, -0.2));
    for (var o = 0; o < 4; o++) {
        h += (vnoise2(q + dirs[o] * t * (0.6 + 0.25 * f32(o))) - 0.5) * amp;
        q = q * 2.03 + vec2<f32>(17.0, 9.0);
        amp *= 0.55;
    }
    return h;
}

// Normal perturbation from ripples (finite differences), as an xz tilt.
fn ripple_slope(p: vec2<f32>, t: f32, eps: f32) -> vec2<f32> {
    let h0 = ripple_height(p, t);
    let hx = ripple_height(p + vec2<f32>(eps, 0.0), t);
    let hz = ripple_height(p + vec2<f32>(0.0, eps), t);
    return vec2<f32>(hx - h0, hz - h0) / eps;
}

// Patchy, streaky foam texture (0..1) at world xz.
fn foam_texture(p: vec2<f32>, t: f32) -> f32 {
    let a = vnoise2(p * 9.0 + vec2<f32>(t * 0.3, 0.0));
    let b = vnoise2(p * 23.0 - vec2<f32>(0.0, t * 0.2));
    let c = vnoise2(p * 3.5 + vec2<f32>(5.0, t * 0.1));
    return smoothstep(0.35, 0.75, a * 0.5 + b * 0.3 + c * 0.4);
}
