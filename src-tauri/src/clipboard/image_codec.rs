//! 剪贴板图片快照转码（跨平台）：导出「保存图片」时按目标扩展名重编码。
//!
//! 截图类应用常只写 CF_DIB → 快照落成 .bmp；导出为 .png 时在此转码。
//! 32bpp DIB 的 alpha 常是 GDI 未初始化的全 0，直接转会得到全透明 PNG，
//! 故 BMP 源 alpha 全 0 时按不透明处理（PNG 源不动）。

/// 快照字节按目标扩展名转码（png/bmp；格式一致则原样返回）
pub fn transcode_image_bytes(bytes: &[u8], dest_ext: &str) -> Result<Vec<u8>, String> {
    let src_is_bmp = bytes.len() > 2 && &bytes[0..2] == b"BM";
    let src_is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let src = if src_is_bmp {
        "bmp"
    } else if src_is_png {
        "png"
    } else {
        return Err("图片快照不是 BMP/PNG 格式".into());
    };
    let dest = dest_ext.trim_start_matches('.').to_ascii_lowercase();
    if dest.is_empty() || src == dest {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes).map_err(|e| format!("解码图片失败: {}", e))?;
    let img = if src_is_bmp {
        normalize_bmp_alpha(img)
    } else {
        img
    };
    let fmt = match dest.as_str() {
        "png" => image::ImageFormat::Png,
        "bmp" => image::ImageFormat::Bmp,
        other => return Err(format!("不支持的导出格式: {}", other)),
    };
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, fmt)
        .map_err(|e| format!("编码图片失败: {}", e))?;
    Ok(out.into_inner())
}

/// 图片字节（png/jpg/webp/gif 首帧/bmp/ico）解码为 `(宽, 高, RGBA 像素)`，BMP 源同样做
/// alpha 归一。供非 Windows 写图片剪贴板：arboard 只收 RGBA，笔记图片多为 jpg/webp/gif。
#[cfg(not(target_os = "windows"))]
pub(super) fn decode_rgba(bytes: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("解码图片失败: {}", e))?;
    let img = if bytes.starts_with(b"BM") {
        normalize_bmp_alpha(img)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok((rgba.width() as usize, rgba.height() as usize, rgba.into_raw()))
}

fn normalize_bmp_alpha(img: image::DynamicImage) -> image::DynamicImage {
    let mut rgba = img.to_rgba8();
    if !rgba.pixels().any(|p| p[3] != 0) {
        for p in rgba.pixels_mut() {
            p[3] = 255;
        }
    }
    image::DynamicImage::ImageRgba8(rgba)
}

#[cfg(test)]
pub(in crate::clipboard) mod tests {
    use super::*;

    /// 构造 32bpp BI_RGB 的 BMP 文件字节（自底向上，同 GDI 写剪贴板形态）
    pub(in crate::clipboard) fn bmp32_file(width: u32, height: u32, px: &[[u8; 4]]) -> Vec<u8> {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(width as i32).to_le_bytes());
        dib.extend_from_slice(&(height as i32).to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&(width * height * 4).to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        for row in (0..height as usize).rev() {
            for col in 0..width as usize {
                let [r, g, b, a] = px[row * width as usize + col];
                dib.extend_from_slice(&[b, g, r, a]);
            }
        }
        // BITMAPFILEHEADER
        let off_bits = 14 + 40;
        let file_size = 14 + dib.len();
        let mut out = Vec::with_capacity(file_size);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&(file_size as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(off_bits as u32).to_le_bytes());
        out.extend_from_slice(&dib);
        out
    }

    #[test]
    fn transcode_bmp_to_png_preserves_pixels() {
        let px = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
        ];
        let bmp = bmp32_file(2, 2, &px);
        let png = transcode_image_bytes(&bmp, ".png").expect("转码应成功");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        for (i, e) in px.iter().enumerate() {
            assert_eq!(decoded.get_pixel((i % 2) as u32, (i / 2) as u32).0, *e);
        }
    }

    #[test]
    fn transcode_bmp_zero_alpha_becomes_opaque() {
        let px = [[10, 20, 30, 0], [40, 50, 60, 0], [70, 80, 90, 0], [1, 2, 3, 0]];
        let bmp = bmp32_file(2, 2, &px);
        let png = transcode_image_bytes(&bmp, "png").unwrap();
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        for p in decoded.pixels() {
            assert_eq!(p[3], 255, "alpha 全 0 的 DIB 应按不透明导出");
        }
        assert_eq!(decoded.get_pixel(0, 0).0, [10, 20, 30, 255]);
    }

    #[test]
    fn transcode_same_format_passthrough() {
        let bmp = bmp32_file(2, 2, &[[1, 2, 3, 255]; 4]);
        assert_eq!(transcode_image_bytes(&bmp, "bmp").unwrap(), bmp);

        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let bytes = png.into_inner();
        assert_eq!(transcode_image_bytes(&bytes, ".png").unwrap(), bytes);
    }

    /// 笔记图片常见的 jpg/webp/gif 都要能解成 RGBA（Linux 写剪贴板只收 RGBA）
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn decode_rgba_handles_note_image_formats() {
        for fmt in [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::WebP,
            image::ImageFormat::Gif,
        ] {
            let mut buf = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(3, 2)
                .to_rgb8()
                .write_to(&mut buf, fmt)
                .unwrap();
            let (w, h, rgba) = decode_rgba(&buf.into_inner())
                .unwrap_or_else(|e| panic!("{fmt:?} 应能解码: {e}"));
            assert_eq!((w, h, rgba.len()), (3, 2, 3 * 2 * 4), "{fmt:?}");
        }
    }

    /// BMP 源（Linux 剪贴板历史快照）沿用 alpha 归一；坏字节明确报错，不静默写坏数据
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn decode_rgba_bmp_alpha_and_garbage() {
        let bmp = bmp32_file(2, 2, &[[10, 20, 30, 0]; 4]);
        let (w, h, rgba) = decode_rgba(&bmp).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(&rgba[0..4], &[10, 20, 30, 255], "alpha 全 0 的 DIB 应按不透明");
        assert!(decode_rgba(b"not an image").is_err());
    }
}
