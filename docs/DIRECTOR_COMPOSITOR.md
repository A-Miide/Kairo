# Director mode on the GPU: the native compositor

Owner's brief (2026-10-31): Director mode should get the same wins as OBS. On by default (the
owner's call: the app is young enough to make the change now), with the CPU path as the fallback
where there is no usable GPU, and `KAIRO_GPU_COMPOSITOR=0` to turn it off on a machine that has
trouble. GPU errors are logged, never fatal.

## Why

Today every picture Kairo puts out is `display.html` drawn by WebKit. That keeps themes, fonts
and ProPresenter layouts identical everywhere, but WebKit gives no GPU surface back, so:

1. **Each output window opens the camera again** at full size (`display_video.js`); the operator
   window shares one capture (`video_hub.js`), separate windows can't.
2. **NDI / Syphon / DeckLink go through the CPU**: a 1080p page snapshot (~64 ms) taken twice
   (over black and white, for transparency), CPU fades, a CPU upload to Syphon (`capture.rs`).
3. **Video backgrounds** on those outputs are decoded by ffmpeg, converted to BGRA and laid
   under the snapshot on the CPU, 30 times a second (`video_bg.rs`).
4. **Director's Output view renders the page a second time** instead of showing the output's
   frame.

OBS's model: every source is a GPU texture, one render thread composites the scene at a fixed
rate, only visible sources cost anything, video is decoded and encoded by the video engine,
queues are bounded, buffers are pooled, and drops are counted by cause.

## Shape

A new crate, `src-tauri/crates/kairo-compositor` (wgpu: Metal on macOS, Direct3D 12 on
Windows, Vulkan on Linux; no Tauri in it), owns the GPU and one **canvas** per output size.

```
 sources (latest frame wins, never queued)         one render thread per canvas, fixed fps
 ┌──────────────────────────────┐                 ┌───────────────────────────────────┐
 │ VideoFile  ffmpeg -hwaccel   │──NV12──┐        │ scene: ordered layers             │
 │            auto → NV12 frames│        ├──GPU──▶│  (source, rect, crop, opacity)    │──▶ outputs
 │ Camera     ffmpeg avfoundation/       │ upload │ transition: cut / fade A→B        │
 │            dshow/v4l2 → NV12 │────────┘        │ NV12→RGB and blending in shaders  │
 │ Page       WebKit snapshot on change   ─BGRA──▶│                                   │
 └──────────────────────────────┘    (premultiplied, only when the page changes)       │
                                                                                      ▼
   outputs: readback (double-buffered, async) → NDI (UYVY or BGRA) / DeckLink / streaming
            later: Syphon by shared texture, screens by native windows, Director previews
```

- **Upload, not convert:** sources hand over NV12 (what decoders and cameras produce); the GPU
  converts. That halves the bytes moved and removes ffmpeg's BGRA conversion (most of the CPU
  that remained after hardware decoding, measured 2026-10-31).
- **The page is a layer**, drawn by WebKit only when it changes, as today; the compositor puts
  it over the video every frame on the GPU instead of the CPU.
- **Only what is on a canvas is pulled**: a source with no layer showing it is closed after a
  short linger, like `video_hub.js`.
- **Bounded everything**: each source keeps one latest frame; outputs read back into two
  pooled buffers; nothing grows.
- **Drops counted by cause** (source late, render late, output late), as OBS's stats do.

## Phases

1. **Core** (this branch first): the crate, the scene, NV12 and BGRA layers, fades, async
   readback; video backgrounds for NDI / Syphon / DeckLink rendered through it instead of
   `video_bg.rs`'s CPU mixing.
2. **Cameras**: one native capture per device for the whole app (ffmpeg's avfoundation / dshow
   / v4l2 input, NV12), shared by every canvas, replacing a capture per window.
3. **Outputs**: screens as native windows presenting the canvas, Syphon from the GPU texture,
   streaming fed NV12 from the canvas to the hardware encoder.
4. **Director**: Preview and Program as native views over the operator window's boxes, showing
   the canvases themselves (no second page render).

Each phase compiles and runs on its own, with the old path kept until it is tested.

## Constraints

- Cross-platform from the start (wgpu), so Windows gets every phase.
- Don't raise memory: frames are pooled; GPU textures replace CPU copies rather than adding to
  them; Windows footprint must not grow.
- Every phase keeps the old path as its fallback.
