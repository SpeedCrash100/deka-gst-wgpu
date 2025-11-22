
@group(0) @binding(0)
var input: texture_2d<f32>;

@group(0) @binding(1)
var output: texture_storage_2d<rgba8unorm, write>;


@compute @workgroup_size(8, 8)
fn computeSobel(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x == 0 || id.y == 0 {
        return;
    }

    let kernel = mat3x3<f32>(
        vec3<f32>(1.0, 2.0, 1.0),
        vec3<f32>(0.0, 0.0, 0.0),
        vec3<f32>(-1.0, -2.0, -1.0)
    );

    let color = abs(
        kernel[0][0] * textureLoad(input, vec2<u32>(id.x - 1u, id.y - 1u), 0).rgb
        + kernel[1][0] * textureLoad(input, vec2<u32>(id.x - 1u, id.y + 0u), 0).rgb
        + kernel[2][0] * textureLoad(input, vec2<u32>(id.x - 1u, id.y + 1u), 0).rgb
        + kernel[0][1] * textureLoad(input, vec2<u32>(id.x + 0u, id.y - 1u), 0).rgb
        + kernel[1][1] * textureLoad(input, vec2<u32>(id.x + 0u, id.y + 0u), 0).rgb
        + kernel[2][1] * textureLoad(input, vec2<u32>(id.x + 0u, id.y + 1u), 0).rgb
        + kernel[0][2] * textureLoad(input, vec2<u32>(id.x + 1u, id.y - 1u), 0).rgb
        + kernel[1][2] * textureLoad(input, vec2<u32>(id.x + 1u, id.y + 0u), 0).rgb
        + kernel[2][2] * textureLoad(input, vec2<u32>(id.x + 1u, id.y + 1u), 0).rgb
    );

    let color_clamped = vec3f(
        clamp(color.r, 0.0, 1.0),
        clamp(color.g, 0.0, 1.0),
        clamp(color.b, 0.0, 1.0)
    );

    textureStore(output, id.xy, vec4f(color_clamped, 1.0));
}