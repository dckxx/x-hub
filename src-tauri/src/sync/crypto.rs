//! 云同步客户端加密基元（docs/sync-protocol.md §4）。
//!
//! 参数必须与小程序端 `x-hub-miniprogram/src/sync/crypto.ts` 逐字节一致；
//! 跨端锚点是 `test_vectors.json`（noble 生成，本模块 `#[test]` 断言）。
//! 口令是唯一根（ADR 0002/0003）：master → record_key（不出设备）+ recovery_prf（唯一离机值）。

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use scrypt::{scrypt as scrypt_kdf, Params};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// scrypt N = 2^15 = 32768（与小程序 KDF_PARAMS 对齐）
const SCRYPT_LOG_N: u8 = 15;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;
const MASTER_LEN: usize = 64;

const HKDF_RECORD_INFO: &[u8] = b"xhub-sync-record-v1";
const HKDF_AUTH_INFO: &[u8] = b"xhub-sync-auth-v1";
pub const NONCE_LEN: usize = 24; // XChaCha

// ---------------- 编码 ----------------

pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 严格解码 hex；非法输入返回 None **而不是 panic**——喂进来的是钥匙串里的值，
/// 被外部改写/截断时不能让命令线程崩掉（下面 load_* 一律按「没有密钥」降级处理）
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for i in 0..bytes.len() / 2 {
        let hi = (bytes[i * 2] as char).to_digit(16)?;
        let lo = (bytes[i * 2 + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}


pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex_encode(&h.finalize())
}

// ---------------- 派生 ----------------

/// 口令归一化：先去首尾空白，再 NFC（与小程序 normalizePassphrase 一致）
pub fn normalize_passphrase(passphrase: &str) -> String {
    passphrase.trim().nfc().collect()
}

/// scrypt 派生 master（同步阻塞数十毫秒级，桌面端可接受）
pub fn derive_master(passphrase: &str, salt: &[u8]) -> [u8; MASTER_LEN] {
    let mut out = [0u8; MASTER_LEN];
    // 第 4 参是派生密钥长度（10..=64），内存预算由 crate 内部按 N/r/p 计算
    let params = Params::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P, MASTER_LEN as usize)
        .expect("scrypt params");
    scrypt_kdf(normalize_passphrase(passphrase).as_bytes(), salt, &params, &mut out)
        .expect("scrypt derive");
    out
}

/// 空间维度再分化：不同空间即使口令相同，钥匙也不同
pub fn derive_record_key(master: &[u8], space_id: &str) -> [u8; 32] {
    hkdf_sha256(master, space_id.as_bytes(), HKDF_RECORD_INFO)
}

/// 恢复登录用的单向校验原料（离机前必须 sha256）
pub fn derive_recovery_prf(master: &[u8], space_id: &str) -> [u8; 32] {
    hkdf_sha256(master, space_id.as_bytes(), HKDF_AUTH_INFO)
}

fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm).expect("hkdf expand");
    okm
}

// ---------------- 记录加解密 ----------------

fn record_aad(entity: &str, uid: &str, updated_at: i64) -> Vec<u8> {
    format!("v1|{entity}|{uid}|{updated_at}").into_bytes()
}

/// 加密一条记录载荷；返回 ct||tag
pub fn seal_record(
    record_key: &[u8],
    entity: &str,
    uid: &str,
    updated_at: i64,
    payload_json: &str,
    nonce: &[u8; NONCE_LEN],
) -> Result<Vec<u8>, String> {
    let cipher = XChaCha20Poly1305::new_from_slice(record_key).map_err(|_| "BAD_KEY")?;
    cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: payload_json.as_bytes(),
                aad: &record_aad(entity, uid, updated_at),
            },
        )
        .map_err(|_| "AEAD_SEAL_FAILED".to_string())
}

/// 解密一条记录；口令错误/密文被篡改时返回 Err（AEAD tag 校验失败）
pub fn open_record(
    record_key: &[u8],
    entity: &str,
    uid: &str,
    updated_at: i64,
    nonce: &[u8; NONCE_LEN],
    ciphertext: &[u8],
) -> Result<String, String> {
    let cipher = XChaCha20Poly1305::new_from_slice(record_key).map_err(|_| "BAD_KEY")?;
    let pt = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: &record_aad(entity, uid, updated_at),
            },
        )
        .map_err(|_| "AEAD_OPEN_FAILED".to_string())?;
    String::from_utf8(pt).map_err(|_| "PAYLOAD_NOT_UTF8".to_string())
}

// ---------------- 本机密钥存取（系统钥匙串；与账号 token 同级保护） ----------------

const KEYRING_SERVICE: &str = "x-hub-sync";
const KEY_RECORD: &str = "record-key";
const KEY_RECOVERY: &str = "recovery-prf";
const KEY_DEVICE: &str = "device-token";
const KEY_DEVICE_ID: &str = "device-id";

fn keyring_set(key: &str, value: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, key).map_err(|e| format!("钥匙串不可用: {e}"))?;
    entry.set_password(value).map_err(|e| format!("钥匙串写入失败: {e}"))
}

fn keyring_get(key: &str) -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, key).ok()?.get_password().ok()
}

fn keyring_del(key: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, key) {
        let _ = entry.delete_credential();
    }
}

/// 口令派生后立即落钥匙串（master 不存——要重建就从口令重算）
pub fn store_keys(record_key: &[u8], recovery_prf: &[u8]) -> Result<(), String> {
    keyring_set(KEY_RECORD, &hex_encode(record_key))?;
    keyring_set(KEY_RECOVERY, &hex_encode(recovery_prf))
}

pub fn load_record_key() -> Option<[u8; 32]> {
    let hex = keyring_get(KEY_RECORD)?;
    let v = hex_decode(&hex)?;
    (v.len() == 32).then(|| v.try_into().unwrap())
}

pub fn load_recovery_prf() -> Option<Vec<u8>> {
    keyring_get(KEY_RECOVERY).and_then(|h| hex_decode(&h)).filter(|v| v.len() == 32)
}

pub fn store_device_token(token: &str) -> Result<(), String> {
    keyring_set(KEY_DEVICE, token)
}

pub fn load_device_token() -> Option<String> {
    keyring_get(KEY_DEVICE)
}

pub fn store_device_id(id: &str) -> Result<(), String> {
    keyring_set(KEY_DEVICE_ID, id)
}

pub fn load_device_id() -> Option<String> {
    keyring_get(KEY_DEVICE_ID)
}

pub fn clear_local_keys() {
    for k in [KEY_RECORD, KEY_RECOVERY, KEY_DEVICE, KEY_DEVICE_ID] {
        keyring_del(k);
    }
}

/// 只清设备绑定（设备被服务端撤销 / 会话失效时用）：留 space 绑定与 record_key，
/// 用户重新输入口令即可走 `sync_enable` 的 rotate 路径自愈，不必重建空间
pub fn clear_device_binding() {
    for k in [KEY_DEVICE, KEY_DEVICE_ID] {
        keyring_del(k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 跨端锚点：这份向量由小程序端的 noble 生成（x-hub-miniprogram/scripts/gen-sync-vectors.mjs），
    /// 桌面端必须逐字节复现——KDF 参数、口令归一化（trim + NFC）、HKDF info/salt、AAD 拼法、
    /// 密文长度任何一处漂移都会让「手机加密、桌面解不开」，而且在真机上才发现。
    #[test]
    fn matches_miniprogram_test_vectors() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("test_vectors.json")).expect("test_vectors.json");
        let passphrase = v["input"]["passphrase"].as_str().unwrap();
        let salt = hex_decode(v["input"]["salt_hex"].as_str().unwrap()).unwrap();
        let space_id = v["input"]["space_id"].as_str().unwrap();
        let expect = &v["expect"];

        let master = derive_master(passphrase, &salt);
        assert_eq!(hex_encode(&master), expect["master_hex"].as_str().unwrap(), "scrypt 派生不一致");

        let record_key = derive_record_key(&master, space_id);
        assert_eq!(hex_encode(&record_key), expect["record_key_hex"].as_str().unwrap());

        let prf = derive_recovery_prf(&master, space_id);
        assert_eq!(hex_encode(&prf), expect["recovery_prf_hex"].as_str().unwrap());
        assert_eq!(sha256_hex(&prf), expect["recovery_hash_hex"].as_str().unwrap());

        let rec = &expect["record"];
        let entity = rec["entity"].as_str().unwrap();
        let uid = rec["uid"].as_str().unwrap();
        let updated_at = rec["updated_at"].as_i64().unwrap();
        let payload = rec["payload_json"].as_str().unwrap();
        let nonce: [u8; NONCE_LEN] = hex_decode(rec["nonce_hex"].as_str().unwrap())
            .unwrap()
            .as_slice()
            .try_into()
            .unwrap();
        let ct = hex_decode(rec["ct_hex"].as_str().unwrap()).unwrap();

        // AAD 口径
        assert_eq!(
            String::from_utf8(record_aad(entity, uid, updated_at)).unwrap(),
            rec["aad"].as_str().unwrap()
        );
        // 明文摘要（载荷序列化口径）
        assert_eq!(sha256_hex(payload.as_bytes()), rec["payload_sha256_hex"].as_str().unwrap());
        // 同一个 nonce 必须封出同一串密文（AEAD 是确定性的）
        let sealed = seal_record(&record_key, entity, uid, updated_at, payload, &nonce).unwrap();
        assert_eq!(hex_encode(&sealed), hex_encode(&ct), "密文与小程序端不一致");
        // 反向：能解开手机端产出的密文
        assert_eq!(open_record(&record_key, entity, uid, updated_at, &nonce, &ct).unwrap(), payload);
    }

    #[test]
    fn hex_decode_rejects_garbage_without_panicking() {
        // 钥匙串里的值被外部改写/截断时不能 panic 崩掉命令线程，只能当「没有密钥」
        assert_eq!(hex_decode("abc"), None);
        assert_eq!(hex_decode("zz"), None);
        assert_eq!(hex_decode(""), Some(Vec::new()));
        assert_eq!(hex_decode("00FF"), Some(vec![0x00, 0xFF]));
    }

    #[test]
    fn passphrase_normalization_trims_and_nfc() {
        // 全角空格 / 首尾空白不参与派生；NFC 让「é = e + 组合音标」与预组合形态等价
        assert_eq!(normalize_passphrase("  口令 abc  "), "口令 abc");
        assert_eq!(normalize_passphrase("e\u{0301}"), "\u{00e9}");
    }
}
