// One textured quad per layer. `dst` is the quad in clip space (x0, y0, x1, y1), `uv` the part of
// the source it shows (u0, v0, u1, v1), `params` = (opacity, mix, matrix, full range):
// matrix 0 = BT.709, 1 = BT.601; mix is the fade from `from` to `src` for page layers.
struct Layer {
    dst: vec4<f32>,
    uv: vec4<f32>,
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> layer: Layer;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var plane0: texture_2d<f32>;   // NV12: Y.   BGRA: the picture.
@group(0) @binding(3) var plane1: texture_2d<f32>;   // NV12: UV.  BGRA: the picture faded from.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    // Two triangles: (0,0) (1,0) (0,1) / (0,1) (1,0) (1,1).
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let c = corners[i];
    var out: VsOut;
    out.pos = vec4(mix(layer.dst.xy, layer.dst.zw, c), 0.0, 1.0);
    out.uv = mix(layer.uv.xy, layer.uv.zw, c);
    return out;
}

@fragment
fn fs_nv12(in: VsOut) -> @location(0) vec4<f32> {
    var y = textureSample(plane0, samp, in.uv).r;
    var c = textureSample(plane1, samp, in.uv).rg - vec2(0.5019608);
    if (layer.params.w < 0.5) {
        // Limited range: Y 16–235, Cb/Cr 16–240.
        y = (y - 0.0627451) * 1.1643836;
        c = c * 1.1383929;
    }
    var rgb: vec3<f32>;
    if (layer.params.z < 0.5) {
        rgb = vec3(y + 1.5748 * c.y, y - 0.1873 * c.x - 0.4681 * c.y, y + 1.8556 * c.x);
    } else {
        rgb = vec3(y + 1.402 * c.y, y - 0.344136 * c.x - 0.714136 * c.y, y + 1.772 * c.x);
    }
    let a = layer.params.x;
    return vec4(clamp(rgb, vec3(0.0), vec3(1.0)) * a, a);
}

@fragment
fn fs_bgra(in: VsOut) -> @location(0) vec4<f32> {
    // Premultiplied: a cross-fade of two premultiplied pictures is a plain mix.
    let next = textureSample(plane0, samp, in.uv);
    let prev = textureSample(plane1, samp, in.uv);
    return mix(prev, next, layer.params.y) * layer.params.x;
}
