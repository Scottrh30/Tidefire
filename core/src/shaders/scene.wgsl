// Ground plane + domain bounding boxes

struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    light_dir: vec4<f32>,   // xyz towards light, w = exposure
    viewport: vec4<f32>,    // width, height, time, transparent bg
    bg_color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> cam: Camera;

struct GroundOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
}

@vertex
fn vs_ground(@builtin(vertex_index) vi: u32) -> GroundOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let c = corners[vi] * 12.0;
    let w = vec3<f32>(c.x, 0.0, c.y);
    var o: GroundOut;
    o.clip = cam.view_proj * vec4<f32>(w, 1.0);
    o.world = w;
    return o;
}

@fragment
fn fs_ground(in: GroundOut) -> @location(0) vec4<f32> {
    let p = in.world.xz;
    let grid = abs(fract(p * 2.0 - 0.5) - 0.5) / fwidth(p * 2.0);
    let line = 1.0 - min(min(grid.x, grid.y), 1.0);
    let major = abs(fract(p * 0.5 - 0.5) - 0.5) / fwidth(p * 0.5);
    let mline = 1.0 - min(min(major.x, major.y), 1.0);
    let dist = length(p);
    let fade = 1.0 - smoothstep(4.0, 12.0, dist);
    var col = vec3<f32>(0.012, 0.013, 0.016);
    col += vec3<f32>(0.006) * line * fade + vec3<f32>(0.018) * mline * fade;
    let bg = cam.bg_color.rgb;
    col = mix(bg, col, fade);
    let a = select(1.0, fade, cam.viewport.w > 0.5);
    return vec4<f32>(col * a, a);
}

// ---------------------------------------------------------------- bounds
struct LineIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_line(in: LineIn) -> LineOut {
    var o: LineOut;
    o.clip = cam.view_proj * vec4<f32>(in.pos, 1.0);
    o.color = in.color;
    return o;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color.rgb * in.color.a, in.color.a);
}
