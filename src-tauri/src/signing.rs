//! 清单签名验证（Ed25519 分离签名）。
//!
//! 市场清单 `registry.json` 与（后续）更新清单 `update.json` 发布时用 Ed25519 私钥
//! 对**原始字节**签名，64 字节签名经 base64 编码存为 `.sig` 文本文件与清单并列上传。
//! 客户端只内嵌公钥：验证通过才信任清单内容 —— 清单是唯一安全根，
//! 一切下载物（扩展 zip 包等）的 sha256 均由签名清单背书，客户端不信任任何未签名字节。
//!
//! 私钥仅存在于发布侧（GitHub Secrets / 本地 .x-hub-signing），绝不进入仓库与二进制。

/// 生产公钥：raw 32 字节 Ed25519 公钥的 base64 编码，存于 `src-tauri/keys/market_public.key`。
/// 轮换公钥 = 替换该文件 + 重新编译发版（旧客户端持旧公钥，不认新私钥签的清单，属预期 breaking）。
pub const MARKET_PUBLIC_KEY_B64: &str = include_str!("../keys/market_public.key");

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use ed25519_dalek::Verifier as _;

/// 校验 `content` 的分离签名（`signature_b64` 为 base64 编码的 64 字节 Ed25519 签名）。
pub fn verify_detached(content: &[u8], signature_b64: &str) -> Result<(), String> {
    let pub_bytes = B64
        .decode(MARKET_PUBLIC_KEY_B64.trim())
        .map_err(|e| format!("公钥解码失败: {e}"))?;
    let pub_bytes: [u8; 32] = pub_bytes
        .try_into()
        .map_err(|_| "公钥长度非法：应为 32 字节".to_string())?;
    let key = ed25519_dalek::VerifyingKey::from_bytes(&pub_bytes)
        .map_err(|e| format!("公钥非法: {e}"))?;

    let sig_bytes = B64
        .decode(signature_b64.trim())
        .map_err(|e| format!("签名解码失败: {e}"))?;
    let sig = ed25519_dalek::Signature::from_slice(&sig_bytes)
        .map_err(|e| format!("签名非法: {e}"))?;

    key.verify(content, &sig)
        .map_err(|_| "签名验证失败：清单可能被篡改或来源不可信".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试向量：由发布侧生成的同一 Ed25519 密钥对（对应 MARKET_PUBLIC_KEY_B64）
    // 对固定文本的签名。仅用于验证「公钥 → 验签」链路正确，私钥不落地代码。
    const TEST_DATA: &str = "x-hub market registry test vector v1";
    const TEST_SIGNATURE: &str =
        "Z91eGY0mPfeEChpfyvgEhSXo+BoCTODapsDObnOO9gc74OjEFRXRMVoZXn1S7XDV1iLVKZ5xPZtG5bpXz0zZDQ==";

    #[test]
    fn valid_signature_passes() {
        assert!(verify_detached(TEST_DATA.as_bytes(), TEST_SIGNATURE).is_ok());
    }

    #[test]
    fn tampered_content_fails() {
        let r = verify_detached("x-hub market registry test vector v2".as_bytes(), TEST_SIGNATURE);
        assert!(r.is_err(), "内容被篡改后签名应验证失败");
    }

    #[test]
    fn tampered_signature_fails() {
        // 翻转签名最后一个字符（内容不变，签名变）
        let mut sig = TEST_SIGNATURE.to_string();
        let last = sig.pop().unwrap();
        sig.push(if last == 'A' { 'B' } else { 'A' });
        let r = verify_detached(TEST_DATA.as_bytes(), &sig);
        assert!(r.is_err(), "签名被篡改后应验证失败");
    }

    #[test]
    fn invalid_base64_fails() {
        assert!(verify_detached(TEST_DATA.as_bytes(), "!!!not-base64!!!").is_err());
    }

    #[test]
    fn wrong_length_signature_fails() {
        assert!(verify_detached(TEST_DATA.as_bytes(), "c2hvcnQ=").is_err());
    }

    #[test]
    fn empty_signature_fails() {
        assert!(verify_detached(TEST_DATA.as_bytes(), "").is_err());
    }
}