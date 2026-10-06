//! 速达资源「备注」等本机敏感字段的落盘加密（Windows DPAPI，用户作用域 + 应用熵）。
//!
//! 落盘格式：`dp1:<base64(DPAPI 密文)>`——SQLite 与备份文件里不存在明文。
//! 无 `dp1:` 前缀的历史/手工值按明文原样透传（见 unprotect）。
//! 与钥匙串存放 AI API Key 同一语义：凭据不随数据目录迁移——把数据目录/备份
//! 挪到其它机器或其它 Windows 用户后解不开，读取时按无备注处理，绝不毒化整表加载。

use base64::Engine as _;

/// 落盘前缀：标识 DPAPI 密文
const PREFIX: &str = "dp1:";
/// 应用熵：阻止其它程序按「无熵 DPAPI」直接解开本应用库里的备注
const ENTROPY: &[u8] = b"x-hub/resource-remark/v1";

/// 加密明文 → `dp1:<base64>`；空串由调用方归一为 NULL，不走这里。
pub fn protect(plain: &str) -> Result<String, String> {
    let cipher = dpapi_protect(plain.as_bytes())?;
    Ok(format!(
        "{PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(cipher)
    ))
}

/// 解密落盘值 → 明文。`dp1:` 前缀值解不开（跨机器/跨用户/数据损坏）返回 None；
/// 无前缀值按明文透传（兼容手工改库）。
pub fn unprotect(stored: &str) -> Option<String> {
    let Some(blob) = stored.strip_prefix(PREFIX) else {
        return (!stored.is_empty()).then(|| stored.to_string());
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(blob).ok()?;
    let plain = dpapi_unprotect(&bytes)?;
    String::from_utf8(plain).ok()
}

#[cfg(target_os = "windows")]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let src = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY.len() as u32,
        pbData: ENTROPY.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &src,
            PCWSTR::null(),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
        .map_err(|e| format!("DPAPI 加密失败: {e}"))?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
        Ok(bytes)
    }
}

#[cfg(target_os = "windows")]
fn dpapi_unprotect(cipher: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let src = CRYPT_INTEGER_BLOB {
        cbData: cipher.len() as u32,
        pbData: cipher.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY.len() as u32,
        pbData: ENTROPY.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&src, None, Some(&entropy), None, None, 0, &mut out).ok()?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
        Some(bytes)
    }
}

#[cfg(not(target_os = "windows"))]
fn dpapi_protect(_plain: &[u8]) -> Result<Vec<u8>, String> {
    Err("备注加密仅支持 Windows".into())
}

#[cfg(not(target_os = "windows"))]
fn dpapi_unprotect(_cipher: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_stores_only_ciphertext() {
        let stored = protect("p@ssw0rd 密码123").unwrap();
        assert!(stored.starts_with(PREFIX));
        assert!(!stored.contains("p@ssw0rd"), "落盘值不得含明文");
        assert_eq!(unprotect(&stored).as_deref(), Some("p@ssw0rd 密码123"));
    }

    #[test]
    fn same_plaintext_encrypts_differently() {
        // DPAPI 每次取随机会话密钥：同明文两次落盘密文不同（防密文比对推断）
        let a = protect("same").unwrap();
        let b = protect("same").unwrap();
        assert_ne!(a, b);
        assert_eq!(unprotect(&a).as_deref(), Some("same"));
        assert_eq!(unprotect(&b).as_deref(), Some("same"));
    }

    #[test]
    fn undecryptable_and_plain_values() {
        // 跨机器/跨用户恢复的残值解不开 → None（按无备注处理，不报错）
        let foreign = format!(
            "{PREFIX}{}",
            base64::engine::general_purpose::STANDARD.encode(b"short")
        );
        assert_eq!(unprotect(&foreign), None);
        assert_eq!(unprotect("dp1:!!!not-base64!!!"), None);
        // 无前缀 = 明文透传（手工改库兼容）；空串按无备注
        assert_eq!(unprotect("plain-text").as_deref(), Some("plain-text"));
        assert_eq!(unprotect(""), None);
    }
}
