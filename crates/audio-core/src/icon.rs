use std::ffi::c_void;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

struct IconGuard(HICON);
impl Drop for IconGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DestroyIcon(self.0);
            }
        }
    }
}

struct BitmapGuard(HBITMAP);
impl Drop for BitmapGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.0 .0));
            }
        }
    }
}

struct DcGuard(HDC);
impl Drop for DcGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
}

/// Converts Windows top-down 32-bit BGRA pixels into RGBA in-place/copy.
pub fn bgra_top_down_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for chunk in bgra.chunks_exact(4) {
        rgba.push(chunk[2]);
        rgba.push(chunk[1]);
        rgba.push(chunk[0]);
        rgba.push(chunk[3]);
    }
    rgba
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &b in data {
        s1 = (s1 + u32::from(b)) % MOD;
        s2 = (s2 + s1) % MOD;
    }
    (s2 << 16) | s1
}

fn push_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let crc_start = out.len();
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let crc = crc32(&out[crc_start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes raw 8-bit RGBA pixels into a valid PNG byte stream using stored deflate blocks.
pub fn encode_rgba_to_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let expected = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    if width == 0 || height == 0 || rgba.len() != expected {
        return None;
    }

    let row_bytes = (width as usize) * 4;
    let mut scanlines = Vec::with_capacity((row_bytes + 1) * height as usize);
    for row in rgba.chunks_exact(row_bytes) {
        scanlines.push(0); // filter type 0 (None)
        scanlines.extend_from_slice(row);
    }

    // Wrap scanlines in a valid zlib stream (CMF=0x78, FLG=0x01, stored blocks, Adler32).
    let mut zlib = Vec::with_capacity(scanlines.len() + 32);
    zlib.push(0x78);
    zlib.push(0x01);
    let chunks: Vec<&[u8]> = scanlines.chunks(65_535).collect();
    for (idx, block) in chunks.iter().enumerate() {
        let is_last = idx + 1 == chunks.len();
        zlib.push(if is_last { 0x01 } else { 0x00 });
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&scanlines).to_be_bytes());

    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = 6; // color type: RGBA
    push_png_chunk(&mut png, b"IHDR", &ihdr);
    push_png_chunk(&mut png, b"IDAT", &zlib);
    push_png_chunk(&mut png, b"IEND", &[]);
    Some(png)
}

pub fn encode_base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = if chunk.len() > 1 { u32::from(chunk[1]) } else { 0 };
        let b2 = if chunk.len() > 2 { u32::from(chunk[2]) } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

pub fn rgba_to_png_data_url(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    let png = encode_rgba_to_png(width, height, rgba)?;
    Some(format!("data:image/png;base64,{}", encode_base64(&png)))
}

unsafe fn read_bitmap_bgra(
    hdc: HDC,
    hbm: HBITMAP,
    width: i32,
    height: i32,
) -> Option<Vec<u8>> {
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -height; // top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0;

    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    let lines = GetDIBits(
        hdc,
        hbm,
        0,
        height as u32,
        Some(pixels.as_mut_ptr() as *mut c_void),
        &mut bmi,
        DIB_RGB_COLORS,
    );
    if lines == 0 {
        None
    } else {
        Some(pixels)
    }
}

/// Extracts the Shell icon for `exe_path` and returns it as a `data:image/png;base64,...` URL.
pub fn extract_exe_icon_data_url(exe_path: &str) -> Option<String> {
    let path = Path::new(exe_path);
    if exe_path.is_empty() || !path.is_file() {
        return None;
    }

    let wide: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut shfi = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if ok == 0 || shfi.hIcon.is_invalid() {
        return None;
    }
    let _icon_guard = IconGuard(shfi.hIcon);

    let mut icon_info = ICONINFO::default();
    if unsafe { GetIconInfo(shfi.hIcon, &mut icon_info) }.is_err() {
        return None;
    }
    let _color_guard = BitmapGuard(icon_info.hbmColor);
    let _mask_guard = BitmapGuard(icon_info.hbmMask);

    if icon_info.hbmColor.is_invalid() {
        return None;
    }

    let mut bmp = BITMAP::default();
    let bmp_size = unsafe {
        GetObjectW(
            HGDIOBJ(icon_info.hbmColor.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut _ as *mut c_void),
        )
    };
    if bmp_size == 0 || bmp.bmWidth <= 0 || bmp.bmHeight <= 0 {
        return None;
    }

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        return None;
    }
    let _dc_guard = DcGuard(hdc);

    let mut bgra = unsafe { read_bitmap_bgra(hdc, icon_info.hbmColor, bmp.bmWidth, bmp.bmHeight) }?;

    // Legacy 24-bit icons leave all alpha bytes as 0; recover opacity from hbmMask when present.
    let all_alpha_zero = bgra.chunks_exact(4).all(|c| c[3] == 0);
    if all_alpha_zero {
        if !icon_info.hbmMask.is_invalid() {
            if let Some(mask) =
                unsafe { read_bitmap_bgra(hdc, icon_info.hbmMask, bmp.bmWidth, bmp.bmHeight) }
            {
                for (px, m) in bgra.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                    let is_transparent = m[0] == 255 && m[1] == 255 && m[2] == 255;
                    px[3] = if is_transparent { 0 } else { 255 };
                }
            }
        } else {
            for px in bgra.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
    }

    let rgba = bgra_top_down_to_rgba(&bgra);
    rgba_to_png_data_url(bmp.bmWidth as u32, bmp.bmHeight as u32, &rgba)
}

#[cfg(test)]
mod tests {
    use super::{bgra_top_down_to_rgba, encode_base64, extract_exe_icon_data_url, rgba_to_png_data_url};

    #[test]
    fn bgra_conversion_swaps_red_and_blue_channels() {
        let rgba = bgra_top_down_to_rgba(&[10, 20, 30, 255]);
        assert_eq!(rgba, vec![30, 20, 10, 255]);
    }

    #[test]
    fn base64_and_png_data_url_produce_valid_header() {
        assert_eq!(encode_base64(b"hi"), "aGk=");
        let url = rgba_to_png_data_url(1, 1, &[255, 0, 0, 255]).expect("png url");
        assert!(url.starts_with("data:image/png;base64,iVBORw0KGgo"));
    }

    #[test]
    fn extract_exe_icon_returns_none_for_missing_path() {
        assert!(extract_exe_icon_data_url(r"C:\nonexistent\missing_app.exe").is_none());
        assert!(extract_exe_icon_data_url("").is_none());
    }
}
