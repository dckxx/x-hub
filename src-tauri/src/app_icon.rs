//! 应用/文件图标提取（高分辨率，取代旧 PowerShell System.Drawing 方案）。
//!
//! 旧方案 `[System.Drawing.Icon]::ExtractAssociatedIcon()` 只能拿到 32×32 位图：
//! 前端图标格子 42–46 CSS px，150%/200% DPI 下要 63–92 物理像素，只能放大渲染，
//! 高分屏上必然发糊。本模块改走 Shell 原生 COM，进程内提取、不再起子进程：
//! - 图标：`IShellItemImageFactory::GetImage(256, SIIGBF_ICONONLY)`——Shell 的标准
//!   取图标入口，exe / dll / 文件夹 / 任意文件的关联图标都能取；现代应用自带
//!   256×256 帧原样返回，老程序最大帧不足 256 时由 Shell 缩放补足（观感不劣于
//!   浏览器放大 32px 源，只会更好）；
//! - `.lnk` 目标解析：`IShellLinkW` + `IPersistFile`，替代 PowerShell WScript.Shell
//!   ——拖入导入不再为取个目标路径单独起一个 PowerShell（省约 300–500ms）；
//! - 位图落地：HBITMAP → `GetDIBits` 32bpp 顶朝下 → 预乘 alpha 还原 → `image` 编码 PNG。
//!
//! COM 单元进出沿用 `service.rs::firewall_com` 的口径：`CoInitializeEx` 回 S_FALSE
//! （线程已初始化为同一单元）也配对 `CoUninitialize`，`RPC_E_CHANGED_MODE`（线程
//! 已在别的单元）复用现成单元、不配对——同步命令跑在已 STA 的主线程时恰好走
//! S_FALSE 平衡路径，不会把应用的 COM 引用计数拆穿。

/// 提取尺寸（像素）：256 覆盖 2× DPI 下的 46px 格子并给更大展示位留余量
#[cfg(target_os = "windows")]
const ICON_SIZE: i32 = 256;

/// 提取程序/文件图标为 PNG 字节（Shell 关联图标，256×256）。
/// 任何失败返回 None——图标是锦上添花，调用方回退旧缓存或名称首字母。
#[cfg(target_os = "windows")]
pub fn extract_icon_png(path: &str) -> Option<Vec<u8>> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::DeleteObject;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY,
    };

    let entered = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let out = (|| {
        let factory: IShellItemImageFactory =
            unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()? };
        let hbm = unsafe {
            factory
                .GetImage(
                    SIZE {
                        cx: ICON_SIZE,
                        cy: ICON_SIZE,
                    },
                    SIIGBF_ICONONLY,
                )
                .ok()?
        };
        let png = unsafe { hbitmap_to_png(hbm) };
        let _ = unsafe { DeleteObject(hbm.into()) };
        png
    })();
    if entered {
        unsafe { CoUninitialize() };
    }
    out
}

/// 解析 `.lnk` 快捷方式的目标路径（IShellLink COM，替代 PowerShell WScript.Shell）。
/// UWP 等无路径目标的快捷方式目标为空 → Err（与旧链路口径一致，调用方拒收）。
#[cfg(target_os = "windows")]
pub fn resolve_lnk_target(lnk_path: &str) -> Result<String, String> {
    use windows::core::{BSTR, Interface, IUnknown};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, STGM_READ,
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

    let entered = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let out = (|| {
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None::<&IUnknown>, CLSCTX_INPROC_SERVER) }
                .map_err(|e| format!("读取快捷方式失败: {e}"))?;
        let pf: IPersistFile = link
            .cast()
            .map_err(|e| format!("读取快捷方式失败: {e}"))?;
        unsafe { pf.Load(&BSTR::from(lnk_path), STGM_READ) }
            .map_err(|e| format!("读取快捷方式失败: {e}"))?;
        let mut buf = [0u16; 260];
        unsafe { link.GetPath(&mut buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32) }
            .map_err(|e| format!("解析快捷方式目标失败: {e}"))?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let target = String::from_utf16_lossy(&buf[..len]).trim().to_string();
        if target.is_empty() {
            return Err("无法解析快捷方式目标路径".into());
        }
        Ok(target)
    })();
    if entered {
        unsafe { CoUninitialize() };
    }
    out
}

#[cfg(target_os = "windows")]
unsafe fn hbitmap_to_png(hbm: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Gdi::{
        GetDC, GetDIBits, GetObjectW, ReleaseDC, BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        DIB_RGB_COLORS,
    };

    let mut bm = BITMAP::default();
    if GetObjectW(
        hbm.into(),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut BITMAP as *mut core::ffi::c_void),
    ) == 0
    {
        return None;
    }
    let (w, h) = (bm.bmWidth.unsigned_abs(), bm.bmHeight.unsigned_abs());
    // GetImage 按 256 请求，异常超大位图（串到缩略图等）直接放弃
    if w == 0 || h == 0 || w > 512 || h > 512 {
        return None;
    }

    let hdc = unsafe { GetDC(None) };
    if hdc.is_invalid() {
        return None;
    }
    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = w as i32;
    // 负高 = 顶朝下 DIB：GetImage 的 HBITMAP 上下颠倒，负高取回来才是正的
    info.bmiHeader.biHeight = -(h as i32);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;
    let mut buf = vec![0u8; w as usize * h as usize * 4];
    let ok = unsafe {
        GetDIBits(
            hdc,
            hbm,
            0,
            h,
            Some(buf.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    } != 0;
    unsafe { ReleaseDC(None, hdc) };
    if !ok {
        return None;
    }

    bgra_premul_to_rgba(&mut buf);
    let img = image::RgbaImage::from_raw(w, h, buf)?;
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

#[cfg(not(target_os = "windows"))]
pub fn extract_icon_png(_path: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn resolve_lnk_target(_lnk_path: &str) -> Result<String, String> {
    Err("仅支持 Windows".into())
}

/// 32bpp DIB 是「预乘 alpha」的 **BGRA** 字节序（Shell 图标位图一贯如此），PNG 要
/// 直通 alpha 的 RGBA：先换 B/R 通道序再去预乘。整图 alpha 全 0 是 DIB 未初始化
/// alpha 的老毛病，按不透明处理（同 clipboard.rs 转码口径）；有有效 alpha 时，
/// 全透明像素的 RGB 残渣清零，防缩放插值边缘串色。
fn bgra_premul_to_rgba(buf: &mut [u8]) {
    let has_alpha = buf.chunks_exact(4).any(|px| px[3] != 0);
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2); // BGRA → RGBA
        let a = px[3] as u32;
        if !has_alpha {
            px[3] = 0xFF;
        } else if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else if a != 0xFF {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// 只读 PNG 文件头（IHDR）取宽度：给「旧 32×32 缓存判旧重提」用，不解码整图。
/// 非 PNG / 读不了返回 None。
pub fn png_width(path: &std::path::Path) -> Option<u32> {
    use std::io::Read;
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 24];
    f.read_exact(&mut head).ok()?;
    if head[..8] != SIG || &head[12..16] != b"IHDR" {
        return None;
    }
    Some(u32::from_be_bytes([head[16], head[17], head[18], head[19]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premul_unwinds_and_opaque_fallback() {
        // BGRA 预乘 (B=32,G=64,R=128)×α128 → RGBA 直通 (255,128,64)；
        // 不透明像素只换通道序
        let mut buf = vec![32u8, 64, 128, 128, 1, 2, 3, 255];
        bgra_premul_to_rgba(&mut buf);
        assert_eq!(&buf[..4], &[255, 128, 64, 128]);
        assert_eq!(&buf[4..], &[3, 2, 1, 255]);

        // 半透明边缘旁的全透明像素：RGB 清零防串色
        let mut buf2 = vec![200u8, 100, 50, 0, 10, 20, 30, 200];
        bgra_premul_to_rgba(&mut buf2);
        assert_eq!(&buf2[..3], &[0, 0, 0]);
        assert_eq!(buf2[3], 0);

        // 整图 alpha 全 0：按不透明处理（通道序照换，RGB 保留）
        let mut buf3 = vec![10u8, 20, 30, 0, 40, 50, 60, 0];
        bgra_premul_to_rgba(&mut buf3);
        assert_eq!(&buf3[..4], &[30, 20, 10, 255]);
        assert_eq!(&buf3[4..], &[60, 50, 40, 255]);
    }

    #[test]
    fn png_width_reads_ihdr() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.png");
        image::RgbaImage::from_raw(37, 19, vec![0u8; 37 * 19 * 4])
            .unwrap()
            .save(&p)
            .unwrap();
        assert_eq!(png_width(&p), Some(37));

        // 非 PNG 一律 None（按无宽度处理，调用方判旧）
        let p2 = dir.path().join("t.bin");
        std::fs::write(&p2, b"not a png at all").unwrap();
        assert_eq!(png_width(&p2), None);
        assert_eq!(png_width(&dir.path().join("absent.png")), None);
    }

    /// 真机链路验证（先例：service::tests::backend_entry_runs_under_real_node）：
    /// 从系统自带 exe 走完整 COM 链路（SHCreateItemFromParsingName → GetImage →
    /// GetDIBits → PNG 编码）提取图标，产物必须可解码且达到请求的 256 宽。
    /// 红/蓝通道是否接反由 premul 单测的通道序锁定，此测不重复断言颜色。
    #[test]
    #[cfg(target_os = "windows")]
    fn extracts_real_icon_end_to_end() {
        let png = extract_icon_png(r"C:\Windows\System32\notepad.exe")
            .expect("notepad.exe 图标提取失败");
        let img = image::load_from_memory(&png).expect("提取产物不是合法 PNG");
        assert!(
            img.width() >= 256 && img.height() >= 256,
            "提取尺寸不足: {}x{}",
            img.width(),
            img.height()
        );
    }

    /// 真机链路验证：解析一个真实存在的系统快捷方式目标。
    #[test]
    #[cfg(target_os = "windows")]
    fn resolves_real_lnk_target() {
        // 写一个指向 notepad.exe 的 .lnk（用 Shell 自己的 COM 接口，测试不依赖桌面现状）
        let dir = tempfile::tempdir().unwrap();
        let lnk = dir.path().join("t.lnk");
        {
            use windows::core::{BSTR, Interface, IUnknown};
            use windows::Win32::System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            };
            use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
            let entered = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
            let r = (|| {
                let link: IShellLinkW = unsafe {
                    CoCreateInstance(&ShellLink, None::<&IUnknown>, CLSCTX_INPROC_SERVER)
                }
                .map_err(|e| e.to_string())?;
                unsafe {
                    link.SetPath(&BSTR::from(r"C:\Windows\System32\notepad.exe"))
                        .map_err(|e| e.to_string())?;
                    let pf: IPersistFile = link.cast().map_err(|e| e.to_string())?;
                    pf.Save(&BSTR::from(lnk.as_os_str().to_string_lossy().as_ref()), false)
                        .map_err(|e| e.to_string())?;
                }
                Ok::<(), String>(())
            })();
            if entered {
                unsafe { CoUninitialize() };
            }
            r.expect("写入测试 .lnk 失败");
        }
        let target = resolve_lnk_target(&lnk.to_string_lossy()).expect("解析 .lnk 失败");
        assert!(
            target.to_ascii_lowercase().ends_with(r"notepad.exe"),
            "解析出的目标不对: {target}"
        );
    }
}
