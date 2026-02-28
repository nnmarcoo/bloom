<div align="center">
  <a href="https://github.com/nnmarcoo/bloom/releases/latest"><img src="assets/logo/banner.png" width="200" alt="bloom"></a>
  <br><br>
  <p><em>hardware-accelerated image viewer built with Rust</em></p>

  ![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-0077aa?style=for-the-badge)
  ![License](https://img.shields.io/badge/license-GPL--3.0-0077aa?style=for-the-badge)
  ![Status](https://img.shields.io/badge/status-alpha-0077aa?style=for-the-badge)
</div>

<!-- <div align="center"><img src="assets/demo.gif" alt="demo" width="800"></div> -->

---

## Features

- **GPU rendering** via [wgpu](https://wgpu.rs) — hardware-accelerated at any resolution
- **Lanczos filtering** for high-quality downsampling with a 5-level mip pyramid
- **Hardware mipmaps** for smooth zoomed-out views
- **GIF, APNG & WebP** animation playback
- **SVG** rendering via resvg
- **Gallery mode** — browse every image in a folder seamlessly
- **Tiled texture system** — handles images larger than GPU texture limits
- Smooth **pan and zoom** with discrete steps from 0.01× to 35×

## Supported Formats

<table align="center">
  <thead>
    <tr><th>Format</th><th>Extension</th><th>Notes</th></tr>
  </thead>
  <tbody>
    <tr><td>JPEG</td><td><code>.jpg</code> <code>.jpeg</code></td><td></td></tr>
    <tr><td>PNG</td><td><code>.png</code></td><td></td></tr>
    <tr><td>Animated PNG</td><td><code>.apng</code></td><td>Animated</td></tr>
    <tr><td>GIF</td><td><code>.gif</code></td><td>Animated</td></tr>
    <tr><td>WebP</td><td><code>.webp</code></td><td>Static and animated</td></tr>
    <tr><td>TIFF</td><td><code>.tif</code> <code>.tiff</code></td><td></td></tr>
    <tr><td>BMP</td><td><code>.bmp</code></td><td></td></tr>
    <tr><td>ICO</td><td><code>.ico</code></td><td></td></tr>
    <tr><td>QOI</td><td><code>.qoi</code></td><td></td></tr>
    <tr><td>Portable bitmap</td><td><code>.pbm</code> <code>.pgm</code> <code>.ppm</code></td><td></td></tr>
    <tr><td>TGA</td><td><code>.tga</code></td><td></td></tr>
    <tr><td>DDS</td><td><code>.dds</code></td><td>BC1–BC7 and uncompressed</td></tr>
    <tr><td>Farbfeld</td><td><code>.ff</code></td><td></td></tr>
    <tr><td>AVIF</td><td><code>.avif</code></td><td></td></tr>
    <tr><td>SVG</td><td><code>.svg</code> <code>.svgz</code></td><td>Rasterized at native size</td></tr>
    <tr><td>HDR (Radiance)</td><td><code>.hdr</code></td><td>Tonemapped (Reinhard)</td></tr>
    <tr><td>OpenEXR</td><td><code>.exr</code></td><td>Tonemapped (Reinhard)</td></tr>
    <tr><td>JPEG XL</td><td><code>.jxl</code></td><td></td></tr>
    <tr><td>JPEG 2000</td><td><code>.jp2</code> <code>.j2k</code> <code>.j2c</code> <code>.jpx</code></td><td></td></tr>
    <tr><td>Photoshop</td><td><code>.psd</code></td><td>Merged composite, no layers</td></tr>
    <tr><td>Krita</td><td><code>.kra</code></td><td>Merged composite, no layers</td></tr>
    <tr><td>Apple Icon</td><td><code>.icns</code></td><td>Largest available size</td></tr>
    <tr><td>DICOM</td><td><code>.dcm</code> <code>.dicom</code></td><td>Medical imaging, first frame</td></tr>
    <tr><td>KTX2</td><td><code>.ktx2</code></td><td>Basis Universal and uncompressed</td></tr>
  </tbody>
</table>

## Default Shortcuts

<table align="center">
  <thead>
    <tr><th>Key</th><th>Action</th></tr>
  </thead>
  <tbody>
    <tr><td><code>←</code> / <code>→</code></td><td>Previous / next image in folder</td></tr>
    <tr><td><code>Ctrl</code> <code>=</code></td><td>Zoom in</td></tr>
    <tr><td><code>Ctrl</code> <code>-</code></td><td>Zoom out</td></tr>
    <tr><td><code>Ctrl</code> <code>0</code></td><td>Fit to window</td></tr>
    <tr><td><code>Ctrl</code> <code>1</code>–<code>9</code></td><td>Fixed zoom (1×–9×)</td></tr>
    <tr><td><code>F</code></td><td>Toggle fullscreen</td></tr>
  </tbody>
</table>

Drag to pan. Scroll wheel to zoom.

## Build

```sh
cargo build --release
```

Requires a GPU with WebGPU support. On Windows, DX12 is used by default.

## Privacy

Bloom is entirely local. It does not collect, transmit, or store any data of any kind. No telemetry, no analytics, no network requests — ever. Your files stay on your machine.

