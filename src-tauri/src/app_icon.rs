//! 应用/文件图标提取（高分辨率，取代旧 PowerShell System.Drawing 方案）。
//!
//! 旧方案 `[System.Drawing.Icon]::ExtractAssociatedIcon()` 只能拿到 32×32 位图：
//! 前端图标格子 42–46 CSS px，150%/200% DPI 下要 63–92 物理像素，只能放大渲染，
//! 高分屏上必然发糊。本模块改走 Shell 原生 COM，进程内提取、不再起子进程：
//! - 图标：`IShellItemImageFactory::GetImage(256, SIIGBF_ICONONLY)`——Shell 的标准
//!   取图标入口，exe / dll / 文件夹 / 任意文件的关联图标都能取；现代应用自带
//!   256×256 帧原样返回。⚠️ **老程序最大帧不足 256 时 Shell 不做放大**：把最大帧
//!   原尺寸居中垫进 256 画布，空位是未初始化的半透明 alpha 垃圾（2026-10-05 修
//!   「速达扫描出的图标变小/像损坏」实测：Cheat Engine 48 帧垫进 256 画布，字形
//!   不透明像素恒为 934 个不随请求尺寸变、垃圾半透明像素随画布增大 164→5172——
//!   「变小」即 48px 字形被 256 画布稀释成 ~19%，「像损坏」即垃圾 alpha 经预乘
//!   还原成深色鬼影）。补偿见下方「小帧垫图补偿」；
//! - `.lnk` 目标解析：`IShellLinkW` + `IPersistFile`，替代 PowerShell WScript.Shell
//!   ——拖入导入不再为取个目标路径单独起一个 PowerShell（省约 300–500ms）；
//! - 位图落地：HBITMAP → `GetDIBits` 32bpp 顶朝下 → 预乘 alpha 还原 → `image` 编码 PNG。
//! - 小帧垫图补偿：256 请求命中垫图时（墨迹包围盒不足画布一半，见 `padded_retry_size`），
//!   按墨迹档位（snap 到 16/24/32/48/64/96/128/256）再提一次——按帧尺寸请求时画布
//!   即被字形铺满（实测 48 帧程序请求 48 得 45×47 墨迹满幅），垫图垃圾也随之消失；
//!   重提仅在「墨迹占比确实变好」时采纳，防稀疏设计的大帧图标被误伤。
//!
//! COM 单元进出沿用 `service.rs::firewall_com` 的口径：`CoInitializeEx` 回 S_FALSE
//! （线程已初始化为同一单元）也配对 `CoUninitialize`，`RPC_E_CHANGED_MODE`（线程
//! 已在别的单元）复用现成单元、不配对——同步命令跑在已 STA 的主线程时恰好走
//! S_FALSE 平衡路径，不会把应用的 COM 引用计数拆穿。

/// 提取尺寸（像素）：256 覆盖 2× DPI 下的 46px 格子并给更大展示位留余量
#[cfg(target_os = "windows")]
const ICON_SIZE: i32 = 256;

/// 墨迹判定阈值（alpha）：小帧垫图的空位垃圾实测 alpha ≤ 77、真字形核心 > 200，
/// 取偏高居中的 160 分离两者（偏取高值防个别源的垫图垃圾更「实」串进墨迹）。
/// 垫图垃圾与本阈值是 `ink_box`/`icon_png_padded` 能识别鬼影缓存的前提，勿调低。
const INK_ALPHA: u8 = 160;

/// 墨迹包围盒任一边不足画布对应边此比例 → 判定「小帧居中垫图」，按档位重提。
/// 正常满幅图标的字形墨迹占画布 ~85-100%（含图标自带的内边距），0.5 留足裕量。
const PADDED_INK_RATIO: f64 = 0.5;

/// 垫图重提的尺寸档位（图标系统的原生帧尺寸）：吸附档位后请求恰为帧尺寸，
/// Shell 直接返回原生帧铺满画布，避免对帧做二次缩放。
const SNAP_SIZES: [u32; 8] = [16, 24, 32, 48, 64, 96, 128, 256];

/// 在 RGBA/BGRA 像素缓冲上计算墨迹（alpha ≥ INK_ALPHA）的包围盒，返回
/// (x, y, w, h)。alpha 在两种字节序下都在每像素第 4 字节。无墨迹返回 None。
fn ink_box(w: u32, h: u32, buf: &[u8]) -> Option<(u32, u32, u32, u32)> {
    let (mut minx, mut miny, mut maxx, mut maxy) = (w, h, 0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            if buf[(y * w + x) as usize * 4 + 3] >= INK_ALPHA {
                minx = minx.min(x);
                maxx = maxx.max(x);
                miny = miny.min(y);
                maxy = maxy.max(y);
            }
        }
    }
    if maxx < minx {
        return None;
    }
    Some((minx, miny, maxx - minx + 1, maxy - miny + 1))
}

/// 墨迹最长边占画布最长边的比例（无墨迹记 0）：垫图判定与「重提是否更优」共用。
fn ink_fill_ratio(w: u32, h: u32, buf: &[u8]) -> f64 {
    match ink_box(w, h, buf) {
        Some((_, _, iw, ih)) => iw.max(ih) as f64 / w.max(h) as f64,
        None => 0.0,
    }
}

/// 吸附到 ≥ n 的最小档位（n 超出 256 按 256——上游画布守卫已限制到 ≤512）
fn snap_up(n: u32) -> u32 {
    SNAP_SIZES
        .iter()
        .copied()
        .find(|s| *s >= n)
        .unwrap_or(SNAP_SIZES[SNAP_SIZES.len() - 1])
}

/// 判定 DIB 是否「小帧居中垫图」并给出重提尺寸：墨迹任一边不足画布一半 → 视为
/// 垫图，按墨迹最长边吸附档位重提（按帧尺寸请求画布即被字形铺满）。非垫图或
/// 无墨迹（全透明等，交由既有 opaque 兜底口径）返回 None。纯函数，单测锁定。
fn padded_retry_size(w: u32, h: u32, buf: &[u8]) -> Option<i32> {
    let (_, _, iw, ih) = ink_box(w, h, buf)?;
    let m = iw.max(ih);
    if (m as f64) < PADDED_INK_RATIO * (w.max(h) as f64) {
        Some(snap_up(m) as i32)
    } else {
        None
    }
}

/// 提取程序/文件图标为 PNG 字节（Shell 关联图标；有 256 帧的程序得 256×256，
/// 最大帧不足的程序得按帧尺寸的紧凑画布——见模块文档「小帧垫图补偿」）。
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
        let get = |size: i32| unsafe {
            factory.GetImage(
                SIZE { cx: size, cy: size },
                SIIGBF_ICONONLY,
            )
        };
        let hbm = get(ICON_SIZE).ok()?;
        let mut chosen = unsafe { hbitmap_to_dib(hbm) };
        // 小帧垫图补偿：256 请求垫回的是「原尺寸居中 + 半透明垃圾」的鬼影图，
        // 按墨迹档位重提一次；仅在墨迹占比确实变好时采纳（稀疏设计的大帧图标
        // 按比例缩放后墨迹占比不变，不采纳、保留原 256 产物）。
        if let Some((buf, w, h)) = chosen.as_ref() {
            let base_ratio = ink_fill_ratio(*w, *h, buf);
            if let Some(size) = padded_retry_size(*w, *h, buf) {
                if let Ok(hbm2) = get(size) {
                    let retry = unsafe { hbitmap_to_dib(hbm2) };
                    let _ = unsafe { DeleteObject(hbm2.into()) };
                    if retry
                        .as_ref()
                        .map(|(b, w, h)| ink_fill_ratio(*w, *h, b) > base_ratio)
                        .unwrap_or(false)
                    {
                        chosen = retry;
                    }
                }
            }
        }
        let _ = unsafe { DeleteObject(hbm.into()) };
        let (mut buf, w, h) = chosen?;
        bgra_premul_to_rgba(&mut buf);
        let img = image::RgbaImage::from_raw(w, h, buf)?;
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .ok()?;
        Some(png)
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

/// HBITMAP → 32bpp 顶朝下 DIB 原始字节（BGRA 预乘，未做任何 alpha 处理）+ 尺寸。
/// GetImage 按 256 请求，异常超大位图（串到缩略图等）直接放弃。
#[cfg(target_os = "windows")]
unsafe fn hbitmap_to_dib(
    hbm: windows::Win32::Graphics::Gdi::HBITMAP,
) -> Option<(Vec<u8>, u32, u32)> {
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
    Some((buf, w, h))
}

/// 判定已落盘的图标缓存是否为「小帧居中垫图」劣化产物（画布宽达标、字形墨迹却
/// 只占中间一小块）：缓存判旧/清扫用——v0.7.6 引入的鬼影缓存宽度就是 256，按
/// 宽度判旧永远抓不到，必须解码看墨迹。解码失败也按垫图处理（让调用方重提自愈）；
/// 无墨迹（全透明等异常）不按垫图处理，交由各口径自行兜底。
pub fn icon_png_padded(path: &std::path::Path) -> bool {
    let Ok(img) = image::open(path) else {
        return true;
    };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    match ink_box(w, h, rgba.as_raw()) {
        Some((_, _, iw, ih)) => (iw.max(ih) as f64) < PADDED_INK_RATIO * (w.max(h) as f64),
        None => false,
    }
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

    /// 小帧垫图判定的口径锁定（实测病灶形态的合成复刻）：
    /// 256 画布正中 47px 实心字形 + 四周 alpha 70 垫图垃圾（实测 Cheat Engine 48 帧
    /// 垫进 256 画布：字形不透明、垃圾 alpha 26~77）→ 判垫图、按档位重提 48。
    #[test]
    fn ink_box_and_retry_size() {
        let (w, h) = (256u32, 256u32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for y in 104..151 {
            for x in 104..151 {
                buf[(y * w + x) as usize * 4 + 3] = 255;
            }
        }
        // 四角垫图垃圾（alpha 70 < INK_ALPHA，不得计入墨迹）
        buf[3] = 70;
        buf[((w - 1) as usize) * 4 + 3] = 70;
        let last = ((h - 1) * w + w - 1) as usize * 4 + 3;
        buf[last] = 70;
        assert_eq!(ink_box(w, h, &buf), Some((104, 104, 47, 47)));
        assert_eq!(padded_retry_size(w, h, &buf), Some(48));

        // 满幅字形 → 不重提
        for px in buf.chunks_exact_mut(4) {
            px[3] = 255;
        }
        assert_eq!(padded_retry_size(w, h, &buf), None);

        // 无墨迹（全透明）→ 不重提，交由全透明→不透明的既有兜底
        for px in buf.chunks_exact_mut(4) {
            px[3] = 0;
        }
        assert_eq!(ink_box(w, h, &buf), None);
        assert_eq!(padded_retry_size(w, h, &buf), None);
        assert_eq!(ink_fill_ratio(w, h, &buf), 0.0);
    }

    #[test]
    fn snap_up_buckets() {
        assert_eq!(snap_up(10), 16);
        assert_eq!(snap_up(16), 16);
        assert_eq!(snap_up(17), 24);
        assert_eq!(snap_up(33), 48);
        assert_eq!(snap_up(47), 48);
        assert_eq!(snap_up(48), 48);
        assert_eq!(snap_up(127), 128);
        assert_eq!(snap_up(200), 256);
        assert_eq!(snap_up(300), 256);
    }

    /// 落盘鬼影缓存的识别口径：256 宽垫图 → true；满幅/无墨迹/非 PNG 各归其位
    #[test]
    fn padded_png_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        // 垫图形态：256 画布中间 45px 方块
        let mut img = image::RgbaImage::from_raw(256, 256, vec![0u8; 256 * 256 * 4]).unwrap();
        for y in 100..145 {
            for x in 100..145 {
                img.put_pixel(x, y, image::Rgba([10, 20, 30, 255]));
            }
        }
        let p = dir.path().join("padded.png");
        img.save(&p).unwrap();
        assert!(icon_png_padded(&p));

        // 正常满幅 → false
        let mut full = vec![0u8; 256 * 256 * 4];
        for px in full.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let p2 = dir.path().join("full.png");
        image::RgbaImage::from_raw(256, 256, full)
            .unwrap()
            .save(&p2)
            .unwrap();
        assert!(!icon_png_padded(&p2));

        // 非 PNG / 读不了 → true（调用方重提自愈）
        let p3 = dir.path().join("x.bin");
        std::fs::write(&p3, b"not a png").unwrap();
        assert!(icon_png_padded(&p3));
        assert!(icon_png_padded(&dir.path().join("absent.png")));
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
