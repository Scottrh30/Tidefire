// FFT ocean (Tessendorf): a wind-sea spectrum evolved in time and turned into
// surface displacement by inverse FFTs, for several cascades (tile sizes) at once.
//
//   evolve    h0(k) -> 4 packed complex spectra per cascade at time t
//   fft_rows  in-place inverse FFT along x (one workgroup per row)
//   fft_cols  in-place inverse FFT along z
//   assemble  -> displacement / derivative / foam textures (one array layer per cascade)
//   (ocean_mips.wgsl then builds the mip chain of those textures)
//
// Packing (all outputs are real, so two real fields share one complex FFT):
//   f0 = Dx + i Dz          horizontal (choppy) displacement
//   f1 = Dy + i dDx/dz      height, cross term of the Jacobian
//   f2 = dDy/dx + i dDy/dz  slopes
//   f3 = dDx/dx + i dDz/dz  Jacobian diagonal

const N: u32 = 256u;
const LOG_N: u32 = 8u;
const FIELDS: u32 = 4u;
const PI: f32 = 3.14159265;

struct FftU {
    lengths: vec4<f32>,  // tile size of each cascade (m)
    misc: vec4<f32>,     // time, choppiness lambda, foam decay per update, foam threshold
    misc2: vec4<f32>,    // speed, foam gain, _, _
}

@group(0) @binding(0) var<uniform> U: FftU;
@group(0) @binding(1) var<storage, read> h0: array<vec4<f32>>;          // h0(k), conj(h0(-k))
@group(0) @binding(2) var<storage, read_write> spec: array<vec2<f32>>;  // cascades * 4 * N * N
@group(0) @binding(3) var<storage, read_write> foam_state: array<f32>;  // cascades * N * N
@group(0) @binding(4) var disp_out: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(5) var deriv_out: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(6) var foam_out: texture_storage_2d_array<rgba16float, write>;

fn cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}
fn times_i(a: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(-a.y, a.x);
}

fn field_index(c: u32, f: u32, row: u32, col: u32) -> u32 {
    return ((c * FIELDS + f) * N + row) * N + col;
}

@compute @workgroup_size(8, 8, 1)
fn evolve(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let z = id.y;
    let c = id.z;
    if (x >= N || z >= N) { return; }
    let L = U.lengths[c];
    let k = vec2<f32>(f32(i32(x) - i32(N / 2u)), f32(i32(z) - i32(N / 2u))) * (2.0 * PI / L);
    let kl = length(k);
    let s = h0[(c * N + z) * N + x];
    // deep-water dispersion
    let w = sqrt(9.81 * kl) * U.misc2.x * U.misc.x;
    let e = vec2<f32>(cos(w), sin(w));
    let h = cmul(s.xy, e) + cmul(s.zw, vec2<f32>(e.x, -e.y));
    let ik = select(vec2<f32>(0.0), k / kl, kl > 1e-6);
    // D = -i k/|k| h ; derivatives: d/dx -> i kx
    let dx = times_i(h) * -ik.x;
    let dz = times_i(h) * -ik.y;
    let dydx = times_i(h) * k.x;
    let dydz = times_i(h) * k.y;
    let kk = select(0.0, 1.0 / kl, kl > 1e-6);
    let dxdx = h * (k.x * k.x * kk);
    let dzdz = h * (k.y * k.y * kk);
    let dxdz = h * (k.x * k.y * kk);
    spec[field_index(c, 0u, z, x)] = dx + times_i(dz);
    spec[field_index(c, 1u, z, x)] = h + times_i(dxdz);
    spec[field_index(c, 2u, z, x)] = dydx + times_i(dydz);
    spec[field_index(c, 3u, z, x)] = dxdx + times_i(dzdz);
}

var<workgroup> line: array<vec2<f32>, 256>;

// Radix-2 inverse FFT of `line` in shared memory (input already bit-reversed).
fn fft_line(t: u32) {
    for (var s = 0u; s < LOG_N; s++) {
        workgroupBarrier();
        let half = 1u << s;
        let j = t & (half - 1u);
        let i0 = (t >> s) * (half << 1u) + j;
        let i1 = i0 + half;
        let ang = PI * f32(j) / f32(half); // +2*pi*j/(2*half): inverse transform
        let w = vec2<f32>(cos(ang), sin(ang));
        let a = line[i0];
        let b = cmul(line[i1], w);
        line[i0] = a + b;
        line[i1] = a - b;
    }
    workgroupBarrier();
}

fn bitrev(i: u32) -> u32 {
    return reverseBits(i) >> (32u - LOG_N);
}

// one workgroup per (row, field, cascade)
@compute @workgroup_size(128, 1, 1)
fn fft_rows(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wg: vec3<u32>) {
    let t = lid.x;
    let row = wg.x;
    let f = wg.y;
    let c = wg.z;
    let a = t;
    let b = t + N / 2u;
    line[bitrev(a)] = spec[field_index(c, f, row, a)];
    line[bitrev(b)] = spec[field_index(c, f, row, b)];
    fft_line(t);
    spec[field_index(c, f, row, a)] = line[a];
    spec[field_index(c, f, row, b)] = line[b];
}

@compute @workgroup_size(128, 1, 1)
fn fft_cols(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wg: vec3<u32>) {
    let t = lid.x;
    let col = wg.x;
    let f = wg.y;
    let c = wg.z;
    let a = t;
    let b = t + N / 2u;
    line[bitrev(a)] = spec[field_index(c, f, a, col)];
    line[bitrev(b)] = spec[field_index(c, f, b, col)];
    fft_line(t);
    spec[field_index(c, f, a, col)] = line[a];
    spec[field_index(c, f, b, col)] = line[b];
}

@compute @workgroup_size(8, 8, 1)
fn assemble(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let z = id.y;
    let c = id.z;
    if (x >= N || z >= N) { return; }
    // the spectrum was centred on k = 0, which flips the sign of every other sample
    let sgn = select(1.0, -1.0, ((x + z) & 1u) == 1u);
    let lam = U.misc.y;
    let f0 = spec[field_index(c, 0u, z, x)] * sgn;
    let f1 = spec[field_index(c, 1u, z, x)] * sgn;
    let f2 = spec[field_index(c, 2u, z, x)] * sgn;
    let f3 = spec[field_index(c, 3u, z, x)] * sgn;
    let disp = vec4<f32>(f0.x * lam, f1.x, f0.y * lam, f1.y * lam);
    let deriv = vec4<f32>(f2.x, f2.y, f3.x * lam, f3.y * lam);
    textureStore(disp_out, vec2<i32>(i32(x), i32(z)), i32(c), disp);
    textureStore(deriv_out, vec2<i32>(i32(x), i32(z)), i32(c), deriv);

    // Foam: where this cascade's waves fold (Jacobian below the threshold) foam is
    // made; it then fades slowly, leaving streaks behind the breaking crests.
    let jac = (1.0 + deriv.z) * (1.0 + deriv.w) - disp.w * disp.w;
    let fi = (c * N + z) * N + x;
    let made = clamp((U.misc.w - jac) * U.misc2.y, 0.0, 1.0);
    let f = max(foam_state[fi] * U.misc.z, made);
    foam_state[fi] = f;
    textureStore(foam_out, vec2<i32>(i32(x), i32(z)), i32(c), vec4<f32>(f, jac, 0.0, 0.0));
}
