# 004 — Runs without a GPU: CPU presentation by default

Status: approved in chat (2026-09-27): "needed without GPU, accessible for everyone,
not expensive on resources".

## Problem
- All art is already drawn on the CPU (`render/canvas.rs`); wgpu only uploads the
  canvas and scales it to the window. Yet no GPU adapter → no window, the app quits.
- Starting wgpu loads the graphics driver stacks: ~95 MB resident of the ~120 MB the
  process uses when idle, and it can wake the dedicated GPU on hybrid laptops.
- VMs, remote desktops and old machines often have no usable GPU.

## Approach
- `render/cpu.rs`: presents through `softbuffer` (plain shared-memory blit, no driver).
  Nearest-neighbour upscale on the CPU: each canvas row is scaled once, then copied
  `scale` times, so a full-screen frame is mostly `memcpy`.
- `render/screen.rs`: `Screen` = the presenter a window uses, picked by a pure rule:
  1. CPU whenever the window is opaque, or the CPU path can show alpha (X11, which
     also covers the Wayland overlay via XWayland).
  2. A see-through window elsewhere (native Wayland, macOS, Windows) tries the GPU;
     if that fails too, CPU opaque — the app already degrades to the full scene.
  3. CPU failing falls back to the GPU. `SNOWLEARNER_RENDERER=cpu|gpu` forces one.
- wgpu becomes the `gpu` cargo feature (on by default, pure Rust, no system deps);
  `--no-default-features` builds contain no GPU code at all.

## Non-goals
- Changing any art, the canvas, or window behaviour.

## Test plan
- Upscale: exact pixels at scale 1/2/3, crop when the canvas overhangs the window,
  clear pixels outside the canvas, premultiplied alpha, empty/degenerate sizes.
- Presenter rule: every row of the table above, including the env override and
  builds without `gpu`.
- Measured on the real app: idle RSS/CPU before vs after, window and overlay mode,
  and the process never maps a GPU driver library in the default path.
