// Tidefire — GPU FLIP liquid solver on a staggered (MAC) grid, plus ballistic spray.
//
// Particle positions are in grid units (cells). Velocities are world m/s.
// Node (i,j,k) stores: u on face x=i, v on face y=j, w on face z=k
// (i.e. u at (i, j+.5, k+.5), v at (i+.5, j, k+.5), w at (i+.5, j+.5, k)).
// Cell (i,j,k) data share the same padded node index.

struct WParams {
    res: vec4<u32>,     // nx, ny, nz, particle count
    box_min: vec4<f32>, // world min xyz, cell size h
    misc: vec4<f32>,    // dt, flip ratio, gravity, time
    spray: vec4<f32>,   // spray speed threshold, spawn probability, air drag, lifetime
    spray2: vec4<u32>,  // max spray, frame seed, transfer (0 = APIC, 1 = FLIP), _
    foam: vec4<f32>,    // foam speed threshold, foam decay, density correction, rest count
}

struct Particle {
    pos: vec4<f32>, // grid units, w = foam
    vel: vec4<f32>, // m/s, w = spawn time
    // APIC affine velocity: row c = gradient of velocity component c, in (m/s) per cell
    c0: vec4<f32>,
    c1: vec4<f32>,
    c2: vec4<f32>,
}

struct Spray {
    pos: vec4<f32>, // grid units, w = life remaining (s)
    vel: vec4<f32>, // m/s, w = size jitter
}

const FIX: f32 = 65536.0;
const AIR: f32 = 0.0;
const FLUID: f32 = 1.0;
const SOLID: f32 = 2.0;

@group(0) @binding(0) var<uniform> P: WParams;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(2) var<storage, read_write> accum: array<atomic<i32>>; // 7 per node
@group(0) @binding(3) var<storage, read_write> gvel: array<vec4<f32>>;    // u v w, mask bits
@group(0) @binding(4) var<storage, read_write> gvel_old: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> cell: array<vec4<f32>>;    // pressure, divergence, type, count
@group(0) @binding(6) var<storage, read_write> spray: array<Spray>;
@group(0) @binding(7) var<storage, read_write> counter: array<atomic<u32>>;

fn ndims() -> vec3<i32> { return vec3<i32>(P.res.xyz) + vec3<i32>(1); }
fn cdims() -> vec3<i32> { return vec3<i32>(P.res.xyz); }

fn nidx(c: vec3<i32>) -> u32 {
    let d = ndims();
    return u32(c.x + d.x * (c.y + d.y * c.z));
}

fn node_in(c: vec3<i32>) -> bool {
    return all(c >= vec3<i32>(0)) && all(c < ndims());
}

fn cell_in(c: vec3<i32>) -> bool {
    return all(c >= vec3<i32>(0)) && all(c < cdims());
}

fn cell_type(c: vec3<i32>) -> f32 {
    if (!cell_in(c)) { return SOLID; }
    return cell[nidx(c)].z;
}

fn comp_offset(comp: u32) -> vec3<f32> {
    if (comp == 0u) { return vec3<f32>(0.0, 0.5, 0.5); }
    if (comp == 1u) { return vec3<f32>(0.5, 0.0, 0.5); }
    return vec3<f32>(0.5, 0.5, 0.0);
}

fn rand(seed: u32) -> f32 {
    var x = seed * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    x = (x >> 22u) ^ x;
    return f32(x) / 4294967295.0;
}

// ------------------------------------------------------------ grid clear
@compute @workgroup_size(4, 4, 4)
fn clear_grid(@builtin(global_invocation_id) g: vec3<u32>) {
    let c = vec3<i32>(g);
    if (!node_in(c)) { return; }
    let n = nidx(c);
    for (var k = 0u; k < 7u; k++) {
        atomicStore(&accum[n * 7u + k], 0);
    }
}

// ------------------------------------------------------------ particle -> grid
@compute @workgroup_size(64)
fn p2g(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = g.x;
    if (id >= P.res.w) { return; }
    let p = particles[id];
    let vel = p.vel.xyz;
    let cm = mat3x3<f32>(p.c0.xyz, p.c1.xyz, p.c2.xyz); // column c = gradient of component c
    for (var comp = 0u; comp < 3u; comp++) {
        let grad = cm[comp];
        let q = p.pos.xyz - comp_offset(comp);
        let b = vec3<i32>(floor(q));
        let f = q - floor(q);
        for (var corner = 0u; corner < 8u; corner++) {
            let o = vec3<i32>(i32(corner & 1u), i32((corner >> 1u) & 1u), i32((corner >> 2u) & 1u));
            let nc = b + o;
            if (!node_in(nc)) { continue; }
            let wv = mix(vec3<f32>(1.0) - f, f, vec3<f32>(o));
            let w = wv.x * wv.y * wv.z;
            if (w <= 0.0) { continue; }
            let n = nidx(nc);
            // APIC: the particle's velocity field is affine, evaluated at the node (o - f = node - particle, in cells)
            let vn = vel[comp] + dot(grad, vec3<f32>(o) - f);
            atomicAdd(&accum[n * 7u + comp], i32(round(clamp(vn, -40.0, 40.0) * w * FIX)));
            atomicAdd(&accum[n * 7u + 3u + comp], i32(round(w * FIX)));
        }
    }
    let ci = vec3<i32>(floor(p.pos.xyz));
    if (cell_in(ci)) {
        atomicAdd(&accum[nidx(ci) * 7u + 6u], 1);
    }
}

// ------------------------------------------------------------ normalise + gravity + cell types
@compute @workgroup_size(4, 4, 4)
fn normalize_grid(@builtin(global_invocation_id) g: vec3<u32>) {
    let c = vec3<i32>(g);
    if (!node_in(c)) { return; }
    let n = nidx(c);
    var v = vec3<f32>(0.0);
    var mask = 0u;
    for (var comp = 0u; comp < 3u; comp++) {
        let den = f32(atomicLoad(&accum[n * 7u + 3u + comp]));
        if (den > 0.0) {
            v[comp] = f32(atomicLoad(&accum[n * 7u + comp])) / den;
            mask |= (1u << comp);
        }
    }
    gvel_old[n] = vec4<f32>(v, f32(mask));
    if ((mask & 2u) != 0u) {
        v.y -= P.misc.z * P.misc.x;
    }
    gvel[n] = vec4<f32>(v, f32(mask));

    if (cell_in(c)) {
        let d = cdims();
        let count = f32(atomicLoad(&accum[n * 7u + 6u]));
        var t = AIR;
        if (c.x == 0 || c.x == d.x - 1 || c.y == 0 || c.z == 0 || c.z == d.z - 1) {
            t = SOLID;
        } else if (count > 0.0) {
            t = FLUID;
        }
        cell[n] = vec4<f32>(0.0, 0.0, t, count);
    }
}

// ------------------------------------------------------------ divergence
fn face_vel(c: vec3<i32>, comp: u32) -> f32 {
    // velocity of the face on the negative side of cell c along comp; solid faces are 0
    var o = vec3<i32>(0);
    o[comp] = 1;
    if (cell_type(c) == SOLID || cell_type(c - o) == SOLID) { return 0.0; }
    if (!node_in(c)) { return 0.0; }
    return gvel[nidx(c)][comp];
}

@compute @workgroup_size(4, 4, 4)
fn divergence(@builtin(global_invocation_id) g: vec3<u32>) {
    let c = vec3<i32>(g);
    if (!cell_in(c)) { return; }
    let n = nidx(c);
    var cl = cell[n];
    if (cl.z != FLUID) {
        cell[n] = vec4<f32>(0.0, 0.0, cl.z, cl.w);
        return;
    }
    let h = P.box_min.w;
    let du = face_vel(c + vec3<i32>(1, 0, 0), 0u) - face_vel(c, 0u);
    let dv = face_vel(c + vec3<i32>(0, 1, 0), 1u) - face_vel(c, 1u);
    let dw = face_vel(c + vec3<i32>(0, 0, 1), 2u) - face_vel(c, 2u);
    var div = (du + dv + dw) / h;
    // push particles apart where they have bunched up (volume preservation)
    let rest = P.foam.w;
    let over = max(cl.w / rest - 1.0, 0.0);
    div -= P.foam.z * min(over, 2.0) / max(P.misc.x, 1e-4);
    cell[n] = vec4<f32>(0.0, div, cl.z, cl.w);
}

// ------------------------------------------------------------ pressure (red/black GS)
fn relax(c: vec3<i32>) {
    let n = nidx(c);
    let cl = cell[n];
    if (cl.z != FLUID) { return; }
    let h = P.box_min.w;
    var sum = 0.0;
    var cnt = 0.0;
    for (var a = 0u; a < 6u; a++) {
        var o = vec3<i32>(0);
        let axis = a / 2u;
        if ((a & 1u) == 0u) { o[axis] = 1; } else { o[axis] = -1; }
        let nc = c + o;
        let t = cell_type(nc);
        if (t == SOLID) { continue; }
        cnt += 1.0;
        if (t == FLUID) { sum += cell[nidx(nc)].x; }
    }
    if (cnt == 0.0) { return; }
    let p = (sum - cl.y * h * h) / cnt;
    cell[n] = vec4<f32>(p, cl.y, cl.z, cl.w);
}

@compute @workgroup_size(4, 4, 4)
fn pressure_red(@builtin(global_invocation_id) g: vec3<u32>) {
    let c = vec3<i32>(g);
    if (!cell_in(c) || ((c.x + c.y + c.z) & 1) != 0) { return; }
    relax(c);
}

@compute @workgroup_size(4, 4, 4)
fn pressure_black(@builtin(global_invocation_id) g: vec3<u32>) {
    let c = vec3<i32>(g);
    if (!cell_in(c) || ((c.x + c.y + c.z) & 1) != 1) { return; }
    relax(c);
}

// ------------------------------------------------------------ projection
fn pressure_of(c: vec3<i32>) -> f32 {
    if (cell_type(c) == FLUID) { return cell[nidx(c)].x; }
    return 0.0;
}

@compute @workgroup_size(4, 4, 4)
fn project(@builtin(global_invocation_id) g: vec3<u32>) {
    let c = vec3<i32>(g);
    if (!node_in(c)) { return; }
    let n = nidx(c);
    let h = P.box_min.w;
    var gv = gvel[n];
    var v = gv.xyz;
    var mask = u32(gv.w);
    for (var comp = 0u; comp < 3u; comp++) {
        var o = vec3<i32>(0);
        o[comp] = 1;
        let a = c - o; // cell on the negative side
        let ta = cell_type(a);
        let tb = cell_type(c);
        if (ta == SOLID || tb == SOLID) {
            v[comp] = 0.0;
            mask |= (1u << comp);
        } else if (ta == FLUID || tb == FLUID) {
            v[comp] -= (pressure_of(c) - pressure_of(a)) / h;
            mask |= (1u << comp);
        } else {
            mask &= ~(1u << comp);
        }
    }
    gvel[n] = vec4<f32>(v, f32(mask));
}

// ------------------------------------------------------------ grid -> particle
// APIC: value + gradient (per cell) of one velocity component. Gradient is only
// trusted when all 8 nodes are valid (fully inside the liquid); otherwise zero.
fn interp_grad(p: vec3<f32>, comp: u32) -> vec4<f32> {
    let q = p - comp_offset(comp);
    let b = vec3<i32>(floor(q));
    let f = q - floor(q);
    var sum = 0.0;
    var wsum = 0.0;
    var grad = vec3<f32>(0.0);
    var valid = 0u;
    for (var corner = 0u; corner < 8u; corner++) {
        let o = vec3<i32>(i32(corner & 1u), i32((corner >> 1u) & 1u), i32((corner >> 2u) & 1u));
        let nc = b + o;
        if (!node_in(nc)) { continue; }
        let gv = gvel[nidx(nc)];
        if ((u32(gv.w) & (1u << comp)) == 0u) { continue; }
        let oc = vec3<f32>(o);
        let wv = mix(vec3<f32>(1.0) - f, f, oc);
        let sg = oc * 2.0 - vec3<f32>(1.0); // d(weight)/d(x): +1 towards o = 1, -1 towards o = 0
        let w = wv.x * wv.y * wv.z;
        let dw = vec3<f32>(sg.x * wv.y * wv.z, wv.x * sg.y * wv.z, wv.x * wv.y * sg.z);
        sum += gv[comp] * w;
        wsum += w;
        grad += gv[comp] * dw;
        valid += 1u;
    }
    if (valid == 8u) { return vec4<f32>(grad, sum); }
    if (wsum > 1e-6) { return vec4<f32>(0.0, 0.0, 0.0, sum / wsum); }
    return vec4<f32>(0.0, 0.0, 0.0, 1e30); // marker: no grid info
}

fn interp(p: vec3<f32>, comp: u32, use_old: bool) -> vec2<f32> {
    // returns (value, total valid weight)
    let q = p - comp_offset(comp);
    let b = vec3<i32>(floor(q));
    let f = q - floor(q);
    var sum = 0.0;
    var wsum = 0.0;
    for (var corner = 0u; corner < 8u; corner++) {
        let o = vec3<i32>(i32(corner & 1u), i32((corner >> 1u) & 1u), i32((corner >> 2u) & 1u));
        let nc = b + o;
        if (!node_in(nc)) { continue; }
        var gv: vec4<f32>;
        if (use_old) { gv = gvel_old[nidx(nc)]; } else { gv = gvel[nidx(nc)]; }
        if ((u32(gv.w) & (1u << comp)) == 0u) { continue; }
        let wv = mix(vec3<f32>(1.0) - f, f, vec3<f32>(o));
        let w = wv.x * wv.y * wv.z;
        sum += gv[comp] * w;
        wsum += w;
    }
    if (wsum > 1e-6) { return vec2<f32>(sum / wsum, wsum); }
    return vec2<f32>(0.0, 0.0);
}

fn grid_vel(p: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    var v = fallback;
    for (var comp = 0u; comp < 3u; comp++) {
        let r = interp(p, comp, false);
        if (r.y > 0.0) { v[comp] = r.x; }
    }
    return v;
}

@compute @workgroup_size(64)
fn g2p(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = g.x;
    if (id >= P.res.w) { return; }
    var p = particles[id];
    let dt = P.misc.x;
    let h = P.box_min.w;
    var vp = p.vel.xyz;
    var vpic = vp;
    var vflip = vp;
    var cm = array<vec3<f32>, 3>(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0));
    let apic = P.spray2.z == 0u;
    for (var comp = 0u; comp < 3u; comp++) {
        let ng = interp_grad(p.pos.xyz, comp);
        let od = interp(p.pos.xyz, comp, true);
        if (ng.w < 1e29) {
            vpic[comp] = ng.w;
            cm[comp] = ng.xyz;
            if (od.y > 0.0) {
                vflip[comp] = vp[comp] + (ng.w - od.x);
            } else {
                vflip[comp] = ng.w;
            }
        } else {
            // ballistic: no grid information here
            if (comp == 1u) { vpic[comp] = vp[comp] - P.misc.z * dt; }
            vflip[comp] = vpic[comp];
        }
    }
    var v: vec3<f32>;
    if (apic) {
        v = vpic;
    } else {
        v = mix(vpic, vflip, P.misc.y);
        cm = array<vec3<f32>, 3>(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0));
    }
    let sp = length(v);
    if (sp > 25.0) { v = v * (25.0 / sp); }

    // RK2 advection through the divergence-free grid
    let mid = p.pos.xyz + 0.5 * dt * v / h;
    let vmid = grid_vel(mid, v);
    var pos = p.pos.xyz + dt * vmid / h;

    // collide with the solid walls (one cell thick)
    let d = vec3<f32>(cdims());
    let lo = vec3<f32>(1.001);
    let hi = vec3<f32>(d.x - 1.001, d.y - 0.001, d.z - 1.001);
    if (pos.x < lo.x) { pos.x = lo.x; v.x = max(v.x, 0.0); }
    if (pos.x > hi.x) { pos.x = hi.x; v.x = min(v.x, 0.0); }
    if (pos.y < lo.y) { pos.y = lo.y; v.y = max(v.y, 0.0); }
    if (pos.y > hi.y) { pos.y = hi.y; v.y = min(v.y, 0.0); }
    if (pos.z < lo.z) { pos.z = lo.z; v.z = max(v.z, 0.0); }
    if (pos.z > hi.z) { pos.z = hi.z; v.z = min(v.z, 0.0); }

    // foam / whitewater: driven by speed and by how much the grid changed the particle
    let agitation = length(vflip - vpic) * 1.5 + length(v);
    let foam_target = smoothstep(P.foam.x, P.foam.x * 2.0, agitation);
    let foam = max(p.pos.w * exp(-P.foam.y * dt), foam_target);

    p.pos = vec4<f32>(pos, foam);
    p.vel = vec4<f32>(v, p.vel.w);
    p.c0 = vec4<f32>(cm[0], 0.0);
    p.c1 = vec4<f32>(cm[1], 0.0);
    p.c2 = vec4<f32>(cm[2], 0.0);
    particles[id] = p;

    // spray emission from fast particles at the free surface
    if (P.spray2.x > 0u && length(v) > P.spray.x) {
        let above = vec3<i32>(floor(pos + vec3<f32>(0.0, 1.0, 0.0)));
        if (cell_type(above) == AIR) {
            let seed = id * 9781u + P.spray2.y * 6271u;
            let excess = (length(v) - P.spray.x) / max(P.spray.x, 0.1);
            if (rand(seed) < P.spray.y * min(excess, 2.0)) {
                let slot = atomicAdd(&counter[0], 1u) % P.spray2.x;
                let jit = vec3<f32>(rand(seed + 1u), rand(seed + 2u), rand(seed + 3u)) - vec3<f32>(0.5);
                let sv = v * (1.0 + 0.25 * rand(seed + 4u)) + jit * length(v) * 0.35 + vec3<f32>(0.0, 0.6, 0.0);
                let life = P.spray.w * (0.5 + rand(seed + 5u));
                spray[slot] = Spray(vec4<f32>(pos + jit * 0.5, life), vec4<f32>(sv, rand(seed + 6u)));
            }
        }
    }
}

// ------------------------------------------------------------ spray
@compute @workgroup_size(64)
fn update_spray(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = g.x;
    if (id >= P.spray2.x) { return; }
    var s = spray[id];
    if (s.pos.w <= 0.0) { return; }
    let dt = P.misc.x;
    let h = P.box_min.w;
    var v = s.vel.xyz;
    v.y -= P.misc.z * dt;
    v *= exp(-P.spray.z * dt);
    var pos = s.pos.xyz + v * dt / h;
    var life = s.pos.w - dt;
    let d = vec3<f32>(cdims());
    if (pos.y < 1.0 || pos.x < 1.0 || pos.z < 1.0 || pos.x > d.x - 1.0 || pos.z > d.z - 1.0) {
        life = 0.0;
    }
    // re-absorbed by the body of water
    let c = vec3<i32>(floor(pos));
    if (cell_in(c) && cell_type(c) == FLUID && cell[nidx(c)].w > P.foam.w * 0.75 && v.y < 0.0) {
        life = min(life, 0.0);
    }
    spray[id] = Spray(vec4<f32>(pos, life), vec4<f32>(v, s.vel.w));
}

@compute @workgroup_size(64)
fn clear_spray(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = g.x;
    if (id >= P.spray2.x) { return; }
    spray[id] = Spray(vec4<f32>(0.0), vec4<f32>(0.0));
    if (id == 0u) { atomicStore(&counter[0], 0u); }
}
