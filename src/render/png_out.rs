//! Writes a canvas to PNG, upscaled with nearest-neighbour (snapshots, README art, app icons).

use super::canvas::Canvas;
use anyhow::{Context, Result};
use std::path::Path;

/// The canvas's RGBA pixels with each one turned into a `scale` × `scale` block.
pub fn upscale(canvas: &Canvas, scale: u32) -> Vec<u8> {
    let s = scale.max(1) as usize;
    let (w, h) = (canvas.w as usize, canvas.h as usize);
    let src = canvas.bytes();
    let mut px = vec![0u8; w * s * h * s * 4];
    for y in 0..h * s {
        for x in 0..w * s {
            let si = ((y / s) * w + x / s) * 4;
            let di = (y * w * s + x) * 4;
            px[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    px
}

/// The canvas as PNG bytes, upscaled by `scale` (nearest-neighbour).
pub fn encode(canvas: &Canvas, scale: u32) -> Result<Vec<u8>> {
    let s = scale.max(1) as usize;
    let (w, h) = (canvas.w as usize, canvas.h as usize);
    let px = upscale(canvas, scale);
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, (w * s) as u32, (h * s) as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        writer.write_image_data(&px)?;
        writer.finish()?;
    }
    Ok(out)
}

pub fn write(canvas: &Canvas, scale: u32, path: &Path) -> Result<()> {
    let bytes = encode(canvas, scale)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// Decodes PNG bytes back to (width, height, RGBA pixels): how tests check what was written.
#[cfg(test)]
pub fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let mut px = vec![0u8; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut px).unwrap();
    px.truncate(info.buffer_size());
    (info.width, info.height, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_encoded_canvas_decodes_back_to_its_pixels_scaled_up() {
        let mut c = Canvas::new(3, 2);
        c.set(1, 0, [255, 0, 0, 255]);
        c.set(2, 1, [0, 0, 255, 255]);
        let (w, h, px) = decode(&encode(&c, 4).unwrap());
        assert_eq!((w, h), (12, 8));
        assert_eq!(px, upscale(&c, 4));
        assert_eq!(&px[(4 * 4)..(4 * 4 + 4)], &[255, 0, 0, 255], "pixel (1,0) became a 4x4 block");
    }
}
