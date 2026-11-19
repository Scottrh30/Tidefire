// Tidefire — sparks and embers (GPU particles in world space).
//
// Sparks: launched fast in a cone, fall under gravity, slow down in the air,
// bounce off the ground and cool from white-yellow to red as they age.
// Embers: lifted gently, carried by the fire's smoke currents with a
// swirling wander, flickering as they rise and fade.

struct U {
    emit: vec4<u32>,      // first slot, new sparks, new embers, max particles
    misc: vec4<f32>,      // dt, time, ground height, has smoke (1/0)
    emitter: vec4<f32>,   // position, radius
    spark_dir: vec4<f32>, // direction (normalised), spread half-angle (radians)
    spark: vec4<f32>,     // speed, speed variation, lifetime, gravity
    spark2: vec4<f32>,    // drag, bounce, follow smoke, size jitter
    ember: vec4<f32>,     // lifetime, buoyancy, follow smoke, turbulence
    ember2: vec4<f32>,    // launch speed, _, _, _
    smoke_min: vec4<f32>, // smoke domain min, cell size
    smoke_res: vec4<u32>, // smoke grid size, random seed
}

struct Particle {
    pos: vec4<f32>,  // world position, w = life left (s); <= 0 means dead
    vel: vec4<f32>,  // m/s, w = total life (s)
    info: vec4<f32>, // x: kind (0 spark, 1 ember), y: random 0..1, z: size jitter, w: _
}

@group(0) @binding(0) var<uniform> P: U;
@group(0) @binding(1) var<storage, read_write> parts: array<Particle>;
@group(0) @binding(2) var<storage, read> smoke_vel: array<vec4<f32>>;

fn hash(x_in: u32) -> u32 {
    var x = x_in * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    return (x >> 22u) ^ x;
}

fn rand(seed: u32) -> f32 {
    return f32(hash(seed)) / 4294967295.0;
}

fn rand_sphere(seed: u32) -> vec3<f32> {
    let z = rand(seed) * 2.0 - 1.0;
    let a = rand(seed + 1u) * 6.2831853;
    let r = sqrt(max(1.0 - z * z, 0.0));
    return vec3<f32>(r * cos(a), z, r * sin(a)) * pow(rand(seed + 2u), 0.3333);
}

// ------------------------------------------------------------ smoke velocity lookup
fn smoke_idx(c: vec3<i32>) -> u32 {
    let d = vec3<i32>(P.smoke_res.xyz);
    let q = clamp(c, vec3<i32>(0), d - vec3<i32>(1));
    return u32(q.x + d.x * (q.y + d.y * q.z));
}

fn smoke_at(world: vec3<f32>) -> vec3<f32> {
    if (P.misc.w < 0.5) { return vec3<f32>(0.0); }
    let g = (world - P.smoke_min.xyz) / P.smoke_min.w;
    let d = vec3<f32>(P.smoke_res.xyz);
    if (any(g < vec3<f32>(0.0)) || any(g > d)) { return vec3<f32>(0.0); }
    let f = g - vec3<f32>(0.5);
    let b = vec3<i32>(floor(f));
    let t = f - floor(f);
    let c000 = smoke_vel[smoke_idx(b)].xyz;
    let c100 = smoke_vel[smoke_idx(b + vec3<i32>(1, 0, 0))].xyz;
    let c010 = smoke_vel[smoke_idx(b + vec3<i32>(0, 1, 0))].xyz;
    let c110 = smoke_vel[smoke_idx(b + vec3<i32>(1, 1, 0))].xyz;
    let c001 = smoke_vel[smoke_idx(b + vec3<i32>(0, 0, 1))].xyz;
    let c101 = smoke_vel[smoke_idx(b + vec3<i32>(1, 0, 1))].xyz;
    let c011 = smoke_vel[smoke_idx(b + vec3<i32>(0, 1, 1))].xyz;
    let c111 = smoke_vel[smoke_idx(b + vec3<i32>(1, 1, 1))].xyz;
    return mix(mix(mix(c000, c100, t.x), mix(c010, c110, t.x), t.y),
               mix(mix(c001, c101, t.x), mix(c011, c111, t.x), t.y), t.z);
}

// Smooth pseudo-curl wander field for embers.
fn wander(p: vec3<f32>, t: f32) -> vec3<f32> {
    let q = p * 2.3;
    return vec3<f32>(
        sin(q.y * 1.7 + t * 1.1) + sin(q.z * 2.3 - t * 0.7),
        0.35 * (sin(q.x * 1.3 + t * 0.9) + sin(q.z * 1.9 + t * 1.3)),
        sin(q.x * 2.1 - t * 1.3) + sin(q.y * 1.5 + t * 0.8)) * 0.5;
}

// ------------------------------------------------------------ kernels
@compute @workgroup_size(64)
fn emit(@builtin(global_invocation_id) g: vec3<u32>) {
    let i = g.x;
    let n_spark = P.emit.y;
    let n = n_spark + P.emit.z;
    if (i >= n || P.emit.w == 0u) { return; }
    let slot = (P.emit.x + i) % P.emit.w;
    let seed = hash(P.smoke_res.w ^ (i * 2654435761u));
    var p: Particle;
    let jitter = rand(seed + 7u);
    if (i < n_spark) {
        // spark: random direction inside a cone around spark_dir
        let dir = normalize(P.spark_dir.xyz);
        let cos_max = cos(P.spark_dir.w);
        let cz = mix(cos_max, 1.0, rand(seed + 3u));
        let sz = sqrt(max(1.0 - cz * cz, 0.0));
        let phi = rand(seed + 4u) * 6.2831853;
        let helper = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(dir.y) > 0.9);
        let t1 = normalize(cross(dir, helper));
        let t2 = cross(dir, t1);
        let d = dir * cz + (t1 * cos(phi) + t2 * sin(phi)) * sz;
        let speed = P.spark.x * (1.0 - P.spark.y * rand(seed + 5u));
        let life = P.spark.z * (0.5 + rand(seed + 6u));
        p.pos = vec4<f32>(P.emitter.xyz + rand_sphere(seed + 10u) * P.emitter.w, life);
        p.vel = vec4<f32>(d * speed, life);
        p.info = vec4<f32>(0.0, rand(seed + 8u), 1.0 + (jitter - 0.5) * P.spark2.w, 0.0);
    } else {
        let life = P.ember.x * (0.55 + 0.9 * rand(seed + 6u));
        let up = vec3<f32>(0.0, P.ember2.x * (0.5 + rand(seed + 5u)), 0.0);
        p.pos = vec4<f32>(P.emitter.xyz + rand_sphere(seed + 10u) * P.emitter.w, life);
        p.vel = vec4<f32>(up + rand_sphere(seed + 20u) * 0.3, life);
        p.info = vec4<f32>(1.0, rand(seed + 8u), 0.6 + 0.8 * jitter, 0.0);
    }
    parts[slot] = p;
}

@compute @workgroup_size(64)
fn update(@builtin(global_invocation_id) g: vec3<u32>) {
    let i = g.x;
    if (i >= P.emit.w) { return; }
    var p = parts[i];
    if (p.pos.w <= 0.0) { return; }
    let dt = P.misc.x;
    var pos = p.pos.xyz;
    var v = p.vel.xyz;
    var life = p.pos.w - dt;
    let air = smoke_at(pos);
    if (p.info.x < 0.5) {
        // spark: drag relative to the (partly followed) air, then gravity
        let carried = air * P.spark2.z;
        v = carried + (v - carried) * exp(-P.spark2.x * dt);
        v.y -= P.spark.w * dt;
        pos += v * dt;
        if (pos.y < P.misc.z) {
            pos.y = P.misc.z;
            if (v.y < 0.0) {
                v.y = -v.y * P.spark2.y;
                v.x *= 0.7;
                v.z *= 0.7;
                life -= 0.08; // hitting the ground costs heat
            }
        }
    } else {
        // ember: eases towards the smoke's velocity plus lift and a swirling wander
        let target_v = air * P.ember.z + vec3<f32>(0.0, P.ember.y, 0.0)
            + wander(pos + vec3<f32>(p.info.y * 17.0), P.misc.y + p.info.y * 9.0) * P.ember.w;
        v = mix(v, target_v, 1.0 - exp(-2.5 * dt));
        pos += v * dt;
        if (pos.y < P.misc.z) {
            pos.y = P.misc.z;
            v = vec3<f32>(0.0);
            life -= dt * 2.0; // a landed ember dies out quickly
        }
    }
    parts[i] = Particle(vec4<f32>(pos, life), vec4<f32>(v, p.vel.w), p.info);
}

@compute @workgroup_size(64)
fn clear(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= P.emit.w) { return; }
    parts[g.x] = Particle(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
}
