//! 速达资源「备注」等本机敏感字段的落盘加密。
//!
//! - Windows：DPAPI（用户作用域 + 应用熵），落盘 `dp1:<base64(DPAPI 密文)>`
//! - Linux：系统钥匙串（Secret Service）里存一把随机主密钥，AES-256-GCM 加密，
//!   落盘 `ks1:<base64(nonce ‖ 密文 ‖ tag)>`，应用熵作 AEAD 关联数据
//!
//! SQLite 与备份文件里不存在明文。无已知前缀的历史/手工值按明文原样透传（见 unprotect）。
//! 与钥匙串存放 AI API Key 同一语义：凭据不随数据目录迁移——把数据目录/备份
//! 挪到其它机器、其它用户或另一平台后解不开，读取时按无备注处理，绝不毒化整表加载。

use base64::Engine as _;

/// Windows DPAPI 密文前缀
const DPAPI_PREFIX: &str = "dp1:";
/// Linux 钥匙串主密钥 + AES-256-GCM 密文前缀
const KEYRING_PREFIX: &str = "ks1:";

#[cfg(target_os = "windows")]
const PREFIX: &str = DPAPI_PREFIX;
#[cfg(not(target_os = "windows"))]
const PREFIX: &str = KEYRING_PREFIX;

/// 应用熵：阻止其它程序按「无熵 DPAPI」直接解开本应用库里的备注；
/// Linux 上作 AEAD 关联数据，同样把密文绑定到本用途
const ENTROPY: &[u8] = b"x-hub/resource-remark/v1";

/// 加密明文 → `<平台前缀><base64>`；空串由调用方归一为 NULL，不走这里。
pub fn protect(plain: &str) -> Result<String, String> {
    let cipher = seal(plain.as_bytes())?;
    Ok(format!(
        "{PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(cipher)
    ))
}

/// 解密落盘值 → 明文。本平台密文解不开（跨机器/跨用户/数据损坏）返回 None；
/// 另一平台的密文同样返回 None——不能当明文透传，否则会把密文串原样显示成备注；
/// 无前缀值按明文透传（兼容手工改库）。
pub fn unprotect(stored: &str) -> Option<String> {
    let Some(blob) = stored.strip_prefix(PREFIX) else {
        if stored.starts_with(DPAPI_PREFIX) || stored.starts_with(KEYRING_PREFIX) {
            return None;
        }
        return (!stored.is_empty()).then(|| stored.to_string());
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(blob).ok()?;
    let plain = open(&bytes)?;
    String::from_utf8(plain).ok()
}

#[cfg(target_os = "windows")]
use self::{dpapi_protect as seal, dpapi_unprotect as open};
#[cfg(target_os = "linux")]
use self::keyring_aead::{open, seal};

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

/// Linux：主密钥存系统钥匙串（keyring 在本工程的特性组合下默认后端是 Secret Service，
/// 持久保存；不是重启即清空的内核 keyutils）。钥匙串不可用（无桌面会话/未解锁/无服务）
/// 时加密直接报错，不退回明文落盘——与 Windows DPAPI 失败时同语义。
#[cfg(target_os = "linux")]
mod keyring_aead {
    use super::ENTROPY;
    use base64::Engine as _;
    use openssl::symm::{decrypt_aead, encrypt_aead, Cipher};
    use std::sync::Mutex;

    const KEY_LEN: usize = 32;
    const NONCE_LEN: usize = 12;
    const TAG_LEN: usize = 16;

    /// 成功取到后缓存在进程内：读写备注不必每次走一趟 D-Bus；取失败不缓存，钥匙串解锁后可重试
    static MASTER_KEY: Mutex<Option<[u8; KEY_LEN]>> = Mutex::new(None);

    fn master_key() -> Result<[u8; KEY_LEN], String> {
        let mut cached = MASTER_KEY.lock().map_err(|_| "备注主密钥缓存不可用".to_string())?;
        if let Some(key) = *cached {
            return Ok(key);
        }
        let key = load_or_create_key()?;
        *cached = Some(key);
        Ok(key)
    }

    fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
        let mut buf = [0u8; N];
        getrandom::fill(&mut buf).map_err(|e| format!("生成随机数失败: {e}"))?;
        Ok(buf)
    }

    #[cfg(not(test))]
    fn load_or_create_key() -> Result<[u8; KEY_LEN], String> {
        const KEYRING_SERVICE: &str = "x-hub-secret";
        const KEYRING_USER: &str = "resource-remark-key-v1";
        let b64 = base64::engine::general_purpose::STANDARD;
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| format!("系统钥匙串不可用: {e}"))?;
        match entry.get_password() {
            Ok(stored) => b64
                .decode(stored.trim())
                .ok()
                .and_then(|v| <[u8; KEY_LEN]>::try_from(v).ok())
                .ok_or_else(|| "系统钥匙串里的备注主密钥已损坏".to_string()),
            Err(keyring::Error::NoEntry) => {
                let key = random_bytes::<KEY_LEN>()?;
                entry
                    .set_password(&b64.encode(key))
                    .map_err(|e| format!("写入系统钥匙串失败: {e}"))?;
                log::info!("[secret] 已在系统钥匙串生成备注主密钥");
                Ok(key)
            }
            Err(e) => Err(format!("读取系统钥匙串失败: {e}")),
        }
    }

    /// 单测环境（WSL/CI）没有 Secret Service：用进程内随机密钥，加解密仍走真实 AEAD 路径
    #[cfg(test)]
    fn load_or_create_key() -> Result<[u8; KEY_LEN], String> {
        random_bytes::<KEY_LEN>()
    }

    pub(super) fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
        let key = master_key()?;
        let nonce = random_bytes::<NONCE_LEN>()?;
        let mut tag = [0u8; TAG_LEN];
        let cipher = encrypt_aead(
            Cipher::aes_256_gcm(),
            &key,
            Some(&nonce),
            ENTROPY,
            plain,
            &mut tag,
        )
        .map_err(|e| format!("AES-GCM 加密失败: {e}"))?;
        let mut out = Vec::with_capacity(NONCE_LEN + cipher.len() + TAG_LEN);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&cipher);
        out.extend_from_slice(&tag);
        Ok(out)
    }

    pub(super) fn open(blob: &[u8]) -> Option<Vec<u8>> {
        if blob.len() < NONCE_LEN + TAG_LEN {
            return None;
        }
        let key = master_key().ok()?;
        let (nonce, rest) = blob.split_at(NONCE_LEN);
        let (cipher, tag) = rest.split_at(rest.len() - TAG_LEN);
        decrypt_aead(Cipher::aes_256_gcm(), &key, Some(nonce), ENTROPY, cipher, tag).ok()
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn seal(_plain: &[u8]) -> Result<Vec<u8>, String> {
    Err("当前平台不支持备注加密".into())
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn open(_cipher: &[u8]) -> Option<Vec<u8>> {
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
        // 每次加密取随机量（DPAPI 会话密钥 / AES-GCM nonce）：同明文两次落盘密文不同（防密文比对推断）
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
        assert_eq!(unprotect(&format!("{PREFIX}!!!not-base64!!!")), None);
        // 无前缀 = 明文透传（手工改库兼容）；空串按无备注
        assert_eq!(unprotect("plain-text").as_deref(), Some("plain-text"));
        assert_eq!(unprotect(""), None);
    }

    #[test]
    fn other_platform_ciphertext_is_not_shown_as_plaintext() {
        // Windows 备份恢复到 Linux（或反之）：另一平台的密文解不开，且绝不能原样当备注显示
        let b64 = base64::engine::general_purpose::STANDARD.encode([7u8; 48]);
        for prefix in [DPAPI_PREFIX, KEYRING_PREFIX] {
            if prefix == PREFIX {
                continue;
            }
            assert_eq!(unprotect(&format!("{prefix}{b64}")), None);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tampered_ciphertext_is_rejected() {
        // AES-GCM 带认证：改动任一字节都解不开，不会吐出被篡改的「明文」
        let stored = protect("remark").unwrap();
        let mut raw = base64::engine::general_purpose::STANDARD
            .decode(stored.strip_prefix(PREFIX).unwrap())
            .unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        let tampered = format!(
            "{PREFIX}{}",
            base64::engine::general_purpose::STANDARD.encode(raw)
        );
        assert_eq!(unprotect(&tampered), None);
    }
}
