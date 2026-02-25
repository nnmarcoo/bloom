<div align="center">
  <img src="assets/logo/bloom.svg" width="80" alt="bloom">
  <h1>bloom</h1>
  <p>A fast, GPU-accelerated image viewer built with Rust and WebGPU.</p>

  ![Rust](https://img.shields.io/badge/rust-2024_edition-orange?style=flat-square&logo=rust&logoColor=white)
  ![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-lightgrey?style=flat-square)
  ![License](https://img.shields.io/badge/license-GPL--3.0-blue?style=flat-square)
  ![Status](https://img.shields.io/badge/status-alpha-yellow?style=flat-square)
</div>

<br>

<!-- <div align="center"><img src="assets/demo.gif" alt="demo" width="800"></div> -->

---

## Features

- **GPU rendering** via [wgpu](https://wgpu.rs) — hardware-accelerated at any resolution
- **Lanczos filtering** for high-quality downsampling with a 5-level mip pyramid
- **Hardware mipmaps** for smooth zoomed-out views
- **GIF & animation** playback with per-frame timing
- **Gallery mode** — browse every image in a folder seamlessly
- **Tiled texture system** — handles images larger than GPU texture limits
- Smooth **pan and zoom** with discrete steps from 0.01× to 35×

## Usage

<details>
<summary>Supported Formats</summary>

| Format | Extension |
|--------|-----------|
| JPEG | `.jpg` `.jpeg` |
| PNG | `.png` |
| GIF | `.gif` |
| TIFF | `.tiff` |
| WebP | `.webp` |
| BMP | `.bmp` |
| ICO | `.ico` |
| QOI | `.qoi` |
| Portable bitmap | `.pbm` `.pgm` `.ppm` |

</details>

<details>
<summary>Default Shortcuts</summary>

| Key | Action |
|-----|--------|
| `←` / `→` | Previous / next image in folder |
| `Ctrl` `=` | Zoom in |
| `Ctrl` `-` | Zoom out |
| `Ctrl` `0` | Fit to window |
| `Ctrl` `1`–`9` | Fixed zoom (1×–9×) |
| `F` | Toggle fullscreen |

Drag to pan. Scroll wheel to zoom.

</details>

## Build

```sh
cargo build --release
```

Requires a GPU with WebGPU support. On Windows, DX12 is used by default.

## Stack

- [Iced](https://github.com/iced-rs/iced) — GUI framework
- [wgpu](https://github.com/gfx-rs/wgpu) — GPU rendering
- [image](https://github.com/image-rs/image) — image decoding
- [glam](https://github.com/bitshifter/glam-rs) — math
