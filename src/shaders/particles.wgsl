// Water particles (sphere impostors with real depth) and spray droplets

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

struct PStyle {
    box_min: vec4<f32>,     // xyz, cell size h
    deep: vec4<f32>,        // deep water colour, radius scale
    shallow: vec4<f32>,     // shallow colour, spray radius scale
    foam: vec4<f32>,        // foam colour, foam strength
}

struct Particle { pos: vec4<f32>, vel: vec4<f32>, c0: vec4<f32>, c1: vec4<f32>, c2: vec4<f32> }
struct Drop { pos: vec4<f32>, vel: vec4<f32> }

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<uniform> style: PStyle;
@group(0) @binding(2) var<storage, read> particles: array<Particle>;
@group(0) @binding(3) var<storage, read> spray: array<Drop>;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) center: vec3<f32>,
    @location(2) radius: f32,
    @location(3) foam: f32,
    @location(4) speed: f32,
    @location(5) kind: f32,
}

fn corner(vi: u32) -> vec2<f32> {
    var c = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    return c[vi];
}

fn billboard(center: vec3<f32>, radius: f32, uv: vec2<f32>) -> vec4<f32> {
    // push the quad towards the camera so the sphere's front surface is not clipped
    let to_cam = normalize(cam.cam_pos.xyz - center);
    let w = center + (cam.cam_right.xyz * uv.x + cam.cam_up.xyz * uv.y) * radius + to_cam * radius;
    return cam.view_proj * vec4<f32>(w, 1.0);
}

@vertex
fn vs_water(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let p = particles[ii];
    let h = style.box_min.w;
    let center = style.box_min.xyz + p.pos.xyz * h;
    let r = h * 0.5 * style.deep.w;
    let uv = corner(vi);
    var o: VOut;
    o.clip = billboard(center, r, uv);
    o.uv = uv;
    o.center = center;
    o.radius = r;
    o.foam = p.pos.w;
    o.speed = length(p.vel.xyz);
    o.kind = 0.0;
    return o;
}

@vertex
fn vs_spray(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let s = spray[ii];
    var o: VOut;
    if (s.pos.w <= 0.0) {
        o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0); // culled
        o.uv = vec2<f32>(0.0);
        o.center = vec3<f32>(0.0);
        o.radius = 0.0;
        o.foam = 0.0;
        o.speed = 0.0;
        o.kind = 1.0;
        return o;
    }
    let h = style.box_min.w;
    let center = style.box_min.xyz + s.pos.xyz * h;
    let life = clamp(s.pos.w * 2.0, 0.0, 1.0);
    let r = h * 0.5 * style.shallow.w * (0.5 + s.vel.w) * (0.4 + 0.6 * life);
    let uv = corner(vi);
    o.clip = billboard(center, r, uv);
    o.uv = uv;
    o.center = center;
    o.radius = r;
    o.foam = life;
    o.speed = length(s.vel.xyz);
    o.kind = 1.0;
    return o;
}

struct FOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fs_particle(in: VOut) -> FOut {
    let r2 = dot(in.uv, in.uv);
    if (r2 > 1.0) { discard; }
    let nz = sqrt(1.0 - r2);
    let to_cam = normalize(cam.cam_pos.xyz - in.center);
    let n = normalize(cam.cam_right.xyz * in.uv.x + cam.cam_up.xyz * in.uv.y + to_cam * nz);
    let wp = in.center + n * in.radius;
    let clip = cam.view_proj * vec4<f32>(wp, 1.0);

    let l = normalize(cam.light_dir.xyz);
    let v = to_cam;
    let ndl = max(dot(n, l), 0.0);
    let hv = normalize(l + v);
    let spec = pow(max(dot(n, hv), 0.0), 80.0);
    let fres = pow(1.0 - max(dot(n, v), 0.0), 4.0);

    var base: vec3<f32>;
    if (in.kind < 0.5) {
        let shallow_mix = clamp(in.speed * 0.18, 0.0, 1.0);
        base = mix(style.deep.rgb, style.shallow.rgb, shallow_mix);
        base = mix(base, style.foam.rgb, clamp(in.foam * style.foam.w, 0.0, 1.0));
    } else {
        base = style.foam.rgb;
        // stochastic transparency for fading droplets
        let hsh = fract(sin(dot(in.clip.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453);
        if (hsh > in.foam) { discard; }
    }
    let sky = mix(vec3<f32>(0.08, 0.1, 0.13), vec3<f32>(0.55, 0.65, 0.8), n.y * 0.5 + 0.5);
    var c = base * (0.35 + 0.65 * ndl) + sky * fres * 0.45 + vec3<f32>(spec * 0.8);
    c *= cam.light_dir.w;

    var o: FOut;
    o.color = vec4<f32>(c, 1.0);
    o.depth = clamp(clip.z / clip.w, 0.0, 1.0);
    return o;
}

// ------------------------------------------------------------ screen-space fluid inputs
fn fwd_dir() -> vec3<f32> {
    return normalize(cross(cam.cam_up.xyz, cam.cam_right.xyz));
}

struct DepthOut {
    @location(0) eye_depth: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fs_fluid_depth(in: VOut) -> DepthOut {
    let r2 = dot(in.uv, in.uv);
    if (r2 > 1.0) { discard; }
    let nz = sqrt(1.0 - r2);
    let to_cam = normalize(cam.cam_pos.xyz - in.center);
    let n = cam.cam_right.xyz * in.uv.x + cam.cam_up.xyz * in.uv.y + to_cam * nz;
    let wp = in.center + n * in.radius;
    let clip = cam.view_proj * vec4<f32>(wp, 1.0);
    var o: DepthOut;
    o.eye_depth = vec4<f32>(dot(wp - cam.cam_pos.xyz, fwd_dir()));
    o.depth = clamp(clip.z / clip.w, 0.0, 1.0);
    return o;
}

@fragment
fn fs_thickness(in: VOut) -> @location(0) vec4<f32> {
    let r2 = dot(in.uv, in.uv);
    if (r2 > 1.0) { discard; }
    let t = 2.0 * in.radius * sqrt(1.0 - r2);
    return vec4<f32>(t, in.foam * t, t, 0.0);
}
