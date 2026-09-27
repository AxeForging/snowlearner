//! Writes a canvas to PNG, upscaled with nearest-neighbour (snapshots, README art).

use super::canvas::Canvas;
use anyhow::{Context, Result};
use std::path::Path;

pub fn write(canvas: &Canvas, scale: u32, path: &Path) -> Result<()> {
    let s = scale.max(1) as usize;
    let (w, h) = (canvas.w as usize, canvas.h as usize);
    let src = canvas.bytes();
    let mut out = vec![0u8; w * s * h * s * 4];
    for y in 0..h * s {
        for x in 0..w * s {
            let si = ((y / s) * w + x / s) * 4;
            let di = (y * w * s + x) * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), (w * s) as u32, (h * s) as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&out)?;
    Ok(())
}
