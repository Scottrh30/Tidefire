// Tidefire — GPU smoke / fire solver (collocated grid, semi-Lagrangian, red-black Gauss-Seidel)
//
// Velocity is stored in world units (m/s). Positions inside the solver are in
// grid units (cells), cell centres at i + 0.5.

struct Params {
    res: vec4<u32>,        // nx, ny, nz, frame
    box_min: vec4<f32>,    // world min xyz, cell size h
    emit_pos: vec4<f32>,   // emitter centre (grid units), radius (grid units)
    emit_vel: vec4<f32>,   // emitter velocity (m/s), w = turbulence amount
    emit_rates: vec4<f32>, // fuel, temperature, smoke per second, dt
    combust: vec4<f32>,    // burn rate, heat release, smoke from burn, ignition temp
    diss: vec4<f32>,       // cooling, smoke dissipation, fuel dissipation, velocity damping
    forces: vec4<f32>,     // buoyancy, smoke weight, vorticity, time
    wind: vec4<f32>,       // wind accel xyz, w = emitter on (1/0)
    shape: vec4<f32>,      // emitter noise scale, flicker speed, open top (1/0), _
}

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read_write> vel: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> vel_tmp: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> sc: array<vec4<f32>>;     // smoke, temperature, fuel, reaction
@group(0) @binding(4) var<storage, read_write> sc_tmp: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> pd: array<vec2<f32>>;     // pressure, divergence
@group(0) @binding(6) var<storage, read_write> curl: array<vec4<f32>>;   // curl xyz, |curl|
@group(0) @binding(7) var vol_out: texture_storage_3d<rgba16float, write>;

fn dims() -> vec3<i32> { return vec3<i32>(P.res.xyz); }

fn idx(c: vec3<i32>) -> u32 {
    let d = dims();
    let q = clamp(c, vec3<i32>(0), d - vec3<i32>(1));
    return u32(q.x + d.x * (q.y + d.y * q.z));
}

fn outside(g: vec3<u32>) -> bool {
    return any(g >= P.res.xyz);
}

fn sample_vel(p: vec3<f32>) -> vec3<f32> {
    let f = p - vec3<f32>(0.5);
    let b = vec3<i32>(floor(f));
    let t = f - floor(f);
    let c000 = vel[idx(b)].xyz;
    let c100 = vel[idx(b + vec3<i32>(1, 0, 0))].xyz;
    let c010 = vel[idx(b + vec3<i32>(0, 1, 0))].xyz;
    let c110 = vel[idx(b + vec3<i32>(1, 1, 0))].xyz;
    let c001 = vel[idx(b + vec3<i32>(0, 0, 1))].xyz;
    let c101 = vel[idx(b + vec3<i32>(1, 0, 1))].xyz;
    let c011 = vel[idx(b + vec3<i32>(0, 1, 1))].xyz;
    let c111 = vel[idx(b + vec3<i32>(1, 1, 1))].xyz;
    let x00 = mix(c000, c100, t.x);
    let x10 = mix(c010, c110, t.x);
    let x01 = mix(c001, c101, t.x);
    let x11 = mix(c011, c111, t.x);
    return mix(mix(x00, x10, t.y), mix(x01, x11, t.y), t.z);
}

fn sample_sc(p: vec3<f32>) -> vec4<f32> {
    let f = p - vec3<f32>(0.5);
    let b = vec3<i32>(floor(f));
    let t = f - floor(f);
    let c000 = sc[idx(b)];
    let c100 = sc[idx(b + vec3<i32>(1, 0, 0))];
    let c010 = sc[idx(b + vec3<i32>(0, 1, 0))];
    let c110 = sc[idx(b + vec3<i32>(1, 1, 0))];
    let c001 = sc[idx(b + vec3<i32>(0, 0, 1))];
    let c101 = sc[idx(b + vec3<i32>(1, 0, 1))];
    let c011 = sc[idx(b + vec3<i32>(0, 1, 1))];
    let c111 = sc[idx(b + vec3<i32>(1, 1, 1))];
    let x00 = mix(c000, c100, t.x);
    let x10 = mix(c010, c110, t.x);
    let x01 = mix(c001, c101, t.x);
    let x11 = mix(c011, c111, t.x);
    return mix(mix(x00, x10, t.y), mix(x01, x11, t.y), t.z);
}

// ---------------------------------------------------------------- noise
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

fn fbm(p: vec3<f32>) -> f32 {
    return 0.55 * vnoise(p) + 0.3 * vnoise(p * 2.03 + vec3<f32>(5.2)) + 0.15 * vnoise(p * 4.01 + vec3<f32>(1.7));
}

// ---------------------------------------------------------------- kernels
@compute @workgroup_size(4, 4, 4)
fn emit(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g) || P.wind.w < 0.5) { return; }
    let c = vec3<f32>(g) + vec3<f32>(0.5);
    let r = P.emit_pos.w;
    let d = distance(c, P.emit_pos.xyz);
    if (d >= r) { return; }
    let i = idx(vec3<i32>(g));
    let dt = P.emit_rates.w;
    let t = P.forces.w;
    let fall = 1.0 - smoothstep(r * 0.35, r, d);
    let n = fbm(c * P.shape.x + vec3<f32>(0.0, -t * P.shape.y, t * 0.37));
    let k = fall * clamp(0.25 + 1.5 * n, 0.0, 2.0) * dt;
    var s = sc[i];
    s.z += P.emit_rates.x * k;
    s.y += P.emit_rates.y * k;
    s.x += P.emit_rates.z * k;
    sc[i] = s;
    let turb = P.emit_vel.w * (vec3<f32>(
        fbm(c * 0.21 + vec3<f32>(t, 0.0, 0.0)),
        fbm(c * 0.21 + vec3<f32>(0.0, t, 9.0)),
        fbm(c * 0.21 + vec3<f32>(3.0, 0.0, t))) - vec3<f32>(0.5)) * 2.0;
    let target_v = P.emit_vel.xyz + turb;
    vel[i] = vec4<f32>(mix(vel[i].xyz, target_v, fall * 0.6), 0.0);
}

@compute @workgroup_size(4, 4, 4)
fn advect_vel(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let i = idx(vec3<i32>(g));
    let h = P.box_min.w;
    let dt = P.emit_rates.w;
    let c = vec3<f32>(g) + vec3<f32>(0.5);
    // RK2 backtrace
    let v0 = vel[i].xyz;
    let mid = c - 0.5 * dt * v0 / h;
    let v1 = sample_vel(mid);
    let p = c - dt * v1 / h;
    let damp = exp(-P.diss.w * dt);
    vel_tmp[i] = vec4<f32>(sample_vel(p) * damp, 0.0);
}

@compute @workgroup_size(4, 4, 4)
fn advect_scalars(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let i = idx(vec3<i32>(g));
    let h = P.box_min.w;
    let dt = P.emit_rates.w;
    let c = vec3<f32>(g) + vec3<f32>(0.5);
    let v0 = vel[i].xyz;
    let mid = c - 0.5 * dt * v0 / h;
    let v1 = sample_vel(mid);
    let p = c - dt * v1 / h;
    var s = sample_sc(p);
    var smoke = s.x;
    var temp = s.y;
    var fuel = s.z;
    var react = 0.0;
    // combustion: fuel + heat -> more heat + smoke
    if (fuel > 0.0 && temp > P.combust.w) {
        let burn = min(fuel, P.combust.x * dt * (0.5 + fuel));
        fuel -= burn;
        temp += burn * P.combust.y;
        smoke += burn * P.combust.z;
        react = burn / max(dt, 1e-5);
    }
    temp *= exp(-P.diss.x * dt);
    smoke *= exp(-P.diss.y * dt);
    fuel *= exp(-P.diss.z * dt);
    sc_tmp[i] = vec4<f32>(max(smoke, 0.0), max(temp, 0.0), max(fuel, 0.0), react);
}

@compute @workgroup_size(4, 4, 4)
fn compute_curl(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let c = vec3<i32>(g);
    let h = P.box_min.w;
    let vxp = vel[idx(c + vec3<i32>(1, 0, 0))].xyz;
    let vxm = vel[idx(c - vec3<i32>(1, 0, 0))].xyz;
    let vyp = vel[idx(c + vec3<i32>(0, 1, 0))].xyz;
    let vym = vel[idx(c - vec3<i32>(0, 1, 0))].xyz;
    let vzp = vel[idx(c + vec3<i32>(0, 0, 1))].xyz;
    let vzm = vel[idx(c - vec3<i32>(0, 0, 1))].xyz;
    let inv = 0.5 / h;
    let w = vec3<f32>(
        (vyp.z - vym.z) - (vzp.y - vzm.y),
        (vzp.x - vzm.x) - (vxp.z - vxm.z),
        (vxp.y - vxm.y) - (vyp.x - vym.x)) * inv;
    curl[idx(c)] = vec4<f32>(w, length(w));
}

@compute @workgroup_size(4, 4, 4)
fn forces(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let c = vec3<i32>(g);
    let i = idx(c);
    let h = P.box_min.w;
    let dt = P.emit_rates.w;
    let s = sc[i];
    var f = vec3<f32>(0.0, P.forces.x * s.y - P.forces.y * s.x, 0.0);
    // vorticity confinement
    let gx = curl[idx(c + vec3<i32>(1, 0, 0))].w - curl[idx(c - vec3<i32>(1, 0, 0))].w;
    let gy = curl[idx(c + vec3<i32>(0, 1, 0))].w - curl[idx(c - vec3<i32>(0, 1, 0))].w;
    let gz = curl[idx(c + vec3<i32>(0, 0, 1))].w - curl[idx(c - vec3<i32>(0, 0, 1))].w;
    let grad = vec3<f32>(gx, gy, gz);
    let gl = length(grad);
    if (gl > 1e-6) {
        let n = grad / gl;
        f += P.forces.z * h * cross(n, curl[i].xyz);
    }
    // wind only acts where there is something to push (keeps the domain calm)
    f += P.wind.xyz * clamp(s.x + s.y, 0.0, 1.0);
    var v = vel[i].xyz + f * dt;
    v = apply_walls(c, v);
    vel[i] = vec4<f32>(v, 0.0);
}

fn apply_walls(c: vec3<i32>, v_in: vec3<f32>) -> vec3<f32> {
    let d = dims();
    var v = v_in;
    if (c.x == 0 && v.x < 0.0) { v.x = 0.0; }
    if (c.x == d.x - 1 && v.x > 0.0) { v.x = 0.0; }
    if (c.y == 0 && v.y < 0.0) { v.y = 0.0; }
    if (c.y == d.y - 1 && v.y > 0.0 && P.shape.z < 0.5) { v.y = 0.0; }
    if (c.z == 0 && v.z < 0.0) { v.z = 0.0; }
    if (c.z == d.z - 1 && v.z > 0.0) { v.z = 0.0; }
    return v;
}

@compute @workgroup_size(4, 4, 4)
fn divergence(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let c = vec3<i32>(g);
    let d = dims();
    let h = P.box_min.w;
    // reflective walls: the neighbour across a wall mirrors the normal component
    var xp = vel[idx(c + vec3<i32>(1, 0, 0))].x;
    var xm = vel[idx(c - vec3<i32>(1, 0, 0))].x;
    var yp = vel[idx(c + vec3<i32>(0, 1, 0))].y;
    var ym = vel[idx(c - vec3<i32>(0, 1, 0))].y;
    var zp = vel[idx(c + vec3<i32>(0, 0, 1))].z;
    var zm = vel[idx(c - vec3<i32>(0, 0, 1))].z;
    let vc = vel[idx(c)].xyz;
    if (c.x == d.x - 1) { xp = -vc.x; }
    if (c.x == 0) { xm = -vc.x; }
    if (c.y == d.y - 1 && P.shape.z < 0.5) { yp = -vc.y; }
    if (c.y == 0) { ym = -vc.y; }
    if (c.z == d.z - 1) { zp = -vc.z; }
    if (c.z == 0) { zm = -vc.z; }
    let div = ((xp - xm) + (yp - ym) + (zp - zm)) * 0.5 / h;
    let i = idx(c);
    pd[i] = vec2<f32>(pd[i].x * 0.8, div); // warm start
}

fn relax(c: vec3<i32>) {
    let d = dims();
    let h = P.box_min.w;
    let i = idx(c);
    var sum = 0.0;
    sum += pd[idx(c + vec3<i32>(1, 0, 0))].x;
    sum += pd[idx(c - vec3<i32>(1, 0, 0))].x;
    sum += pd[idx(c - vec3<i32>(0, 1, 0))].x;
    sum += pd[idx(c + vec3<i32>(0, 0, 1))].x;
    sum += pd[idx(c - vec3<i32>(0, 0, 1))].x;
    if (c.y == d.y - 1 && P.shape.z > 0.5) {
        // open top: Dirichlet p = 0 above
    } else {
        sum += pd[idx(c + vec3<i32>(0, 1, 0))].x;
    }
    let p = (sum - pd[i].y * h * h) / 6.0;
    pd[i] = vec2<f32>(p, pd[i].y);
}

@compute @workgroup_size(4, 4, 4)
fn pressure_red(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let c = vec3<i32>(g);
    if (((c.x + c.y + c.z) & 1) != 0) { return; }
    relax(c);
}

@compute @workgroup_size(4, 4, 4)
fn pressure_black(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let c = vec3<i32>(g);
    if (((c.x + c.y + c.z) & 1) != 1) { return; }
    relax(c);
}

@compute @workgroup_size(4, 4, 4)
fn project(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let c = vec3<i32>(g);
    let d = dims();
    let h = P.box_min.w;
    let i = idx(c);
    let pc = pd[i].x;
    var pxp = pd[idx(c + vec3<i32>(1, 0, 0))].x;
    var pxm = pd[idx(c - vec3<i32>(1, 0, 0))].x;
    var pyp = pd[idx(c + vec3<i32>(0, 1, 0))].x;
    var pym = pd[idx(c - vec3<i32>(0, 1, 0))].x;
    var pzp = pd[idx(c + vec3<i32>(0, 0, 1))].x;
    var pzm = pd[idx(c - vec3<i32>(0, 0, 1))].x;
    if (c.y == d.y - 1 && P.shape.z > 0.5) { pyp = 0.0; }
    let grad = vec3<f32>(pxp - pxm, pyp - pym, pzp - pzm) * 0.5 / h;
    var v = vel[i].xyz - grad; // pressure already absorbs dt (lap p = div u)
    v = apply_walls(c, v);
    vel[i] = vec4<f32>(v, 0.0);
}

@compute @workgroup_size(4, 4, 4)
fn pack_volume(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let s = sc[idx(vec3<i32>(g))];
    textureStore(vol_out, vec3<i32>(g), s);
}

@compute @workgroup_size(4, 4, 4)
fn clear_all(@builtin(global_invocation_id) g: vec3<u32>) {
    if (outside(g)) { return; }
    let i = idx(vec3<i32>(g));
    vel[i] = vec4<f32>(0.0);
    vel_tmp[i] = vec4<f32>(0.0);
    sc[i] = vec4<f32>(0.0);
    sc_tmp[i] = vec4<f32>(0.0);
    pd[i] = vec2<f32>(0.0);
    curl[i] = vec4<f32>(0.0);
}
