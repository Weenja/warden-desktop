//! Pixel fix-ups for Windows icons, without Win32, so the tests run on any OS.
//!
//! GDI hands out 32-bit pixels as B, G, R, A. Icons made before Windows XP have no alpha
//! channel (every A byte is 0) and say which pixels are see-through in a separate mask.

/// Turn GDI's colour and mask pixels into PNG-ready R, G, B, A.
/// `mask` is the icon's AND mask read as 32-bit pixels: white = see-through, black = opaque.
/// It is only used when the colour pixels carry no alpha at all.
pub fn icon_rgba(mut color: Vec<u8>, mask: &[u8]) -> Vec<u8> {
    let (pixels, _) = color.as_chunks_mut::<4>();
    let has_alpha = pixels.iter().any(|px| px[3] != 0);
    if !has_alpha {
        let (mask, _) = mask.as_chunks::<4>();
        for (px, m) in pixels.iter_mut().zip(mask) {
            px[3] = if m[0] == 0 { 255 } else { 0 };
        }
    }
    for px in pixels.iter_mut() {
        px.swap(0, 2);
    }
    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_blue_and_red_and_keeps_real_alpha() {
        let color = vec![1, 2, 3, 128, 10, 20, 30, 0];
        let mask = vec![255, 255, 255, 0, 0, 0, 0, 0];
        // Alpha present on one pixel: the mask is ignored, the 0-alpha pixel stays clear.
        assert_eq!(icon_rgba(color, &mask), vec![3, 2, 1, 128, 30, 20, 10, 0]);
    }

    #[test]
    fn icons_without_alpha_take_it_from_the_mask() {
        let color = vec![1, 2, 3, 0, 10, 20, 30, 0];
        let mask = vec![0, 0, 0, 0, 255, 255, 255, 0];
        assert_eq!(icon_rgba(color, &mask), vec![3, 2, 1, 255, 30, 20, 10, 0]);
    }
}
