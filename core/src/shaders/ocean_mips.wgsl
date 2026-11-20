// ---- mip chain: 2x2 box filter from level l-1 into level l
@group(0) @binding(0) var mip_src: texture_2d_array<f32>;
@group(0) @binding(1) var mip_dst: texture_storage_2d_array<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(mip_dst);
    if (id.x >= size.x || id.y >= size.y) { return; }
    let p = vec2<i32>(id.xy) * 2;
    let l = i32(id.z);
    let v = textureLoad(mip_src, p, l, 0) + textureLoad(mip_src, p + vec2<i32>(1, 0), l, 0)
        + textureLoad(mip_src, p + vec2<i32>(0, 1), l, 0) + textureLoad(mip_src, p + vec2<i32>(1, 1), l, 0);
    textureStore(mip_dst, vec2<i32>(id.xy), l, v * 0.25);
}
