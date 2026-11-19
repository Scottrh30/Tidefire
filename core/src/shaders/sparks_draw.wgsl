// Draws sparks (streaks along their motion) and embers (soft glowing dots),
// added on top of the scene (additive light), hidden behind opaque things.

struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    light_dir: vec4<f32>,   // w = exposure
    viewport: vec4<f32>,    // width, height, time, transparent
    bg_color: vec4<f32>,
}

struct Style {
    spark_hot: vec4<f32>,   // rgb, brightness
    spark_cool: vec4<f32>,  // rgb, streak (seconds)
    ember_hot: vec4<f32>,   // rgb, brightness
    ember_cool: vec4<f32>,  // rgb, flickers per second
    size: vec4<f32>,        // spark size (m), ember size (m), pixels per metre at distance 1, time
}

struct Particle {
    pos: vec4<f32>,
    vel: vec4<f32>,
    info: vec4<f32>,
}

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<uniform> S: Style;
@group(0) @binding(2) var<storage, read> parts: array<Particle>;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,   // pixels: x along the streak (0 at tail), y across
    @location(1) len_r: vec2<f32>,   // streak length and radius, pixels
    @location(2) color: vec3<f32>,
}

fn dead() -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    o.local = vec2<f32>(0.0);
    o.len_r = vec2<f32>(0.0, 1.0);
    o.color = vec3<f32>(0.0);
    return o;
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let p = parts[ii];
    if (p.pos.w <= 0.0) { return dead(); }
    let is_spark = p.info.x < 0.5;
    let heat = clamp(p.pos.w / max(p.vel.w, 1e-3), 0.0, 1.0); // 1 = just born, 0 = burnt out
    let head = p.pos.xyz;
    var tail = head;
    if (is_spark) {
        tail = head - p.vel.xyz * S.spark_cool.w;
    }
    let a = cam.view_proj * vec4<f32>(head, 1.0);
    let b = cam.view_proj * vec4<f32>(tail, 1.0);
    if (a.w < 0.02 || b.w < 0.02) { return dead(); }
    let half_vp = cam.viewport.xy * 0.5;
    let sa = a.xy / a.w * half_vp;
    let sb = b.xy / b.w * half_vp;
    let d = sa - sb;
    let len = length(d);
    let dir = select(vec2<f32>(1.0, 0.0), d / max(len, 1e-4), len > 0.25);
    let perp = vec2<f32>(-dir.y, dir.x);
    let size = select(S.size.y, S.size.x, is_spark) * p.info.z;
    // glow radius in pixels: thin cores for sparks, a soft halo for embers; never under ~a pixel
    let r = max(size * S.size.z / a.w, 0.7) * select(2.2, 1.3, is_spark);

    // two triangles spanning the streak, padded by the radius at both ends
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
    let c = corners[vi];
    let at_head = c.x > 0.5;
    let base = select(sb, sa, at_head);
    let along = select(-r, r, at_head);
    let px = base + dir * along + perp * c.y * r;
    let w = select(b.w, a.w, at_head);
    let z = select(b.z / b.w, a.z / a.w, at_head);

    var col: vec3<f32>;
    if (is_spark) {
        col = mix(S.spark_cool.rgb, S.spark_hot.rgb, pow(heat, 0.6)) * S.spark_hot.w;
        col *= smoothstep(0.0, 0.25, heat);
        // a long streak spreads the same light over more pixels
        col *= clamp((2.0 * r) / (len + 2.0 * r), 0.25, 1.0);
    } else {
        let t = S.size.w;
        let flick = 0.6 + 0.25 * sin(t * S.ember_cool.w * 6.2831 + p.info.y * 40.0)
                        + 0.15 * sin(t * S.ember_cool.w * 2.7 + p.info.y * 13.0);
        col = mix(S.ember_cool.rgb, S.ember_hot.rgb, heat) * S.ember_hot.w * flick;
        col *= smoothstep(0.0, 0.3, heat) * smoothstep(1.0, 0.92, heat);
    }

    var o: VOut;
    o.clip = vec4<f32>(px / half_vp * w, z * w, w);
    o.local = vec2<f32>(select(-r, len + r, at_head), c.y * r);
    o.len_r = vec2<f32>(len, r);
    o.color = col * cam.light_dir.w;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let len = in.len_r.x;
    let r = in.len_r.y;
    let dx = max(-in.local.x, 0.0) + max(in.local.x - len, 0.0);
    let dist = length(vec2<f32>(dx, in.local.y)) / r;
    if (dist > 1.0) { discard; }
    let core = exp(-dist * dist * 9.0);
    let glow = exp(-dist * dist * 3.0) * 0.35;
    // soft clip: very bright sparks saturate towards their colour instead of flat white
    let c = vec3<f32>(1.0) - exp(-in.color * (core + glow));
    let a = clamp(max(c.r, max(c.g, c.b)), 0.0, 1.0);
    return vec4<f32>(c, a);
}
