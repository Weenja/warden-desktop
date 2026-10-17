//! App icons for the picker as 64 px PNGs, read from an exe (or dll/ico) with the shell and GDI.

use windows::core::HSTRING;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::Shell::SHDefExtractIconW;
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

use crate::platform::icon_pixels::icon_rgba;

/// Icons in the picker are drawn at 32 px; 64 px keeps them sharp at 200 % scaling.
const ICON_PX: u32 = 64;

/// The icon at `index` in `file` as a PNG, or None when the file has no usable icon.
pub fn exe_icon_png(file: &str, index: i32) -> Option<Vec<u8>> {
    let mut icon = HICON::default();
    // The low word of the size is the large icon's size; the shell scales to it.
    let result = unsafe { SHDefExtractIconW(&HSTRING::from(file), index, 0, Some(&mut icon), None, ICON_PX) };
    if result != S_OK || icon.is_invalid() {
        return None;
    }
    let png = icon_png(icon);
    let _ = unsafe { DestroyIcon(icon) };
    png
}

/// A GDI bitmap we own, deleted on drop.
struct OwnedBitmap(HBITMAP);

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { DeleteObject(self.0.into()) };
        }
    }
}

fn icon_png(icon: HICON) -> Option<Vec<u8>> {
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    // GetIconInfo hands us copies of both bitmaps to delete.
    let color = OwnedBitmap(info.hbmColor);
    let mask = OwnedBitmap(info.hbmMask);
    // Black-and-white icons have no colour bitmap; not worth drawing in the picker.
    if color.0.is_invalid() {
        return None;
    }
    let (width, height) = bitmap_size(color.0)?;
    let pixels = read_pixels(color.0, width, height)?;
    let mask_pixels = read_pixels(mask.0, width, height).unwrap_or_default();
    encode_png(&icon_rgba(pixels, &mask_pixels), width, height)
}

fn bitmap_size(bitmap: HBITMAP) -> Option<(u32, u32)> {
    let mut info = BITMAP::default();
    let written =
        unsafe { GetObjectW(bitmap.into(), size_of::<BITMAP>() as i32, Some(&mut info as *mut BITMAP as *mut _)) };
    (written != 0 && info.bmWidth > 0 && info.bmHeight > 0).then_some((info.bmWidth as u32, info.bmHeight as u32))
}

/// The bitmap's pixels as 32-bit B, G, R, A rows, top row first.
fn read_pixels(bitmap: HBITMAP, width: u32, height: u32) -> Option<Vec<u8>> {
    if bitmap.is_invalid() {
        return None;
    }
    let mut format = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // Negative height = rows top to bottom, the order PNG wants.
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        return None;
    }
    let rows =
        unsafe { GetDIBits(dc, bitmap, 0, height, Some(pixels.as_mut_ptr().cast()), &mut format, DIB_RGB_COLORS) };
    let _ = unsafe { DeleteDC(dc) };
    (rows == height as i32).then_some(pixels)
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
        writer.finish().ok()?;
    }
    Some(png)
}
