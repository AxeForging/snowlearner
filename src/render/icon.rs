//! The app icon files the desktop shows outside a running window (Start menu,
//! app menu), made from the same art as the window icon: `mage::portrait()`.

use super::{canvas::Canvas, png_out};
use anyhow::{Result, ensure};

/// Upscales of the 32-pixel portrait stored in the `.ico`: 32, 64, 128 and
/// 256 pixels. Nearest-neighbour keeps the pixel art sharp; Windows picks the
/// size it needs and shrinks from 32 for the smallest views.
pub const ICO_SCALES: [u32; 4] = [1, 2, 4, 8];

/// Scale of the Linux menu PNG: 256 pixels from the 32-pixel portrait.
pub const PNG_SCALE: u32 = 8;

/// The `.ico` `setup` writes next to `snowlearner.exe`; `install.ps1` points
/// the Start menu shortcut at this name.
pub const ICO_FILE: &str = "snowlearner.ico";

const ICO_HEADER: usize = 6;
const ICO_ENTRY: usize = 16;

/// A Windows `.ico` holding one PNG-compressed image per scale (the layout
/// Windows has read since Vista). Each image must be 256 pixels or less.
pub fn ico(canvas: &Canvas, scales: &[u32]) -> Result<Vec<u8>> {
    let images = scales.iter().map(|&s| png_out::encode(canvas, s)).collect::<Result<Vec<_>>>()?;
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]); // reserved, type 1 = icon
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = ICO_HEADER + ICO_ENTRY * images.len();
    for (&scale, image) in scales.iter().zip(&images) {
        let (w, h) = (canvas.w as u32 * scale.max(1), canvas.h as u32 * scale.max(1));
        ensure!(w <= 256 && h <= 256, "an icon image is at most 256 pixels, not {w}x{h}");
        // 256 is written as 0: the field is one byte.
        out.extend_from_slice(&[(w % 256) as u8, (h % 256) as u8, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in &images {
        out.extend_from_slice(image);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::png_out::{decode, upscale};

    fn u16_at(b: &[u8], i: usize) -> usize {
        u16::from_le_bytes([b[i], b[i + 1]]) as usize
    }

    fn u32_at(b: &[u8], i: usize) -> usize {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
    }

    #[test]
    fn the_ico_holds_the_portrait_at_every_size_pixel_for_pixel() {
        let art = crate::scene::mage::portrait();
        let bytes = ico(&art, &ICO_SCALES).unwrap();
        assert_eq!(&bytes[..4], &[0, 0, 1, 0], "an icon file");
        assert_eq!(u16_at(&bytes, 4), ICO_SCALES.len());
        for (i, &scale) in ICO_SCALES.iter().enumerate() {
            let entry = ICO_HEADER + i * ICO_ENTRY;
            let side = art.w as u32 * scale;
            assert_eq!(bytes[entry] as u32, side % 256, "width byte, 0 meaning 256");
            assert_eq!(u16_at(&bytes, entry + 6), 32, "32 bits per pixel");
            let (len, at) = (u32_at(&bytes, entry + 8), u32_at(&bytes, entry + 12));
            let (w, h, px) = decode(&bytes[at..at + len]);
            assert_eq!((w, h), (side, side));
            assert_eq!(px, upscale(&art, scale), "the {side}px image is the portrait");
        }
    }

    #[test]
    fn an_image_larger_than_an_icon_allows_is_refused() {
        let art = crate::scene::mage::portrait();
        assert!(ico(&art, &[16]).is_err(), "512 pixels does not fit the size byte");
    }
}
