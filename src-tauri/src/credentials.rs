//! 旧明文凭据仅允许迁入系统钥匙串，不再新增明文回退文件。
use std::path::Path;
use std::sync::Mutex;

static MIGRATION_LOCK: Mutex<()> = Mutex::new(());

pub fn migrate(path: &Path, mut save: impl FnMut(&str, &str) -> Result<(), String>) -> Result<(), String> {
    let _guard = MIGRATION_LOCK.lock().map_err(|_| "凭据迁移锁不可用")?;
    if !path.exists() { return Ok(()); }
    let bytes = std::fs::read(path).map_err(|_| "无法读取旧凭据文件")?;
    let values: std::collections::BTreeMap<String, String> = serde_json::from_slice(&bytes)
        .map_err(|_| "旧凭据格式不正确，已保留原文件")?;
    for (key, value) in values { save(&key, &value)?; }
    // 所有条目已安全持久化后，才移除旧文件；失败时保留以便恢复。
    std::fs::remove_file(path).map_err(|_| "凭据已迁入钥匙串，但无法清理旧明文文件，请检查文件权限".into())
}

pub fn migrate_to_keyring(path: &Path, service: &str, fixed_key: Option<&str>) -> Result<(), String> {
    migrate(path, |key, value| {
        let entry = keyring::Entry::new(service, fixed_key.unwrap_or(key)).map_err(|_| "钥匙串不可用")?;
        match entry.get_password() {
            Ok(_) => Ok(()), // 不用旧文件覆盖后来保存的新凭据。
            Err(keyring::Error::NoEntry) => entry.set_password(value).map_err(|_| "旧凭据迁移失败，已保留原文件".into()),
            Err(_) => Err("无法读取钥匙串，已保留旧凭据文件".into()),
        }
    })
}

/// 供应商端点仅接受 http/https；凭据一律走请求头，禁止内嵌在 URL 或携带 # 片段。
pub fn validate_endpoint(url: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(url).map_err(|_| "服务地址格式不正确")?;
    if !(url.scheme() == "http" || url.scheme() == "https")
        || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("服务地址仅支持 http/https，且不得内嵌账号密码或 # 片段".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn security_migration_never_discards_unsaved_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.json");
        std::fs::write(&path, r#"{"a":"测试值","b":"另一测试值"}"#).unwrap();
        assert!(migrate(&path, |_, _| Err("不可用".into())).is_err());
        assert!(path.exists());
        let mut saved = Vec::new();
        migrate(&path, |k, v| { saved.push((k.to_string(), v.to_string())); Ok(()) }).unwrap();
        assert_eq!(saved.len(), 2);
        assert!(!path.exists());
    }
    #[test]
    fn endpoint_accepts_http_and_https_only() {
        for url in [
            "https://example.com/v1",
            "http://example.com/v1",
            "http://192.168.1.10:8000/v1",
            "http://127.0.0.1:11434/v1",
            "http://[::1]:11434/v1",
        ] {
            assert!(validate_endpoint(url).is_ok(), "{url}");
        }
        for url in [
            "ftp://example.com/v1",
            "file:///secret",
            "https://user:pass@example.com/",
            "http://user@example.com/v1",
            "https://example.com/v1#frag",
        ] {
            assert!(validate_endpoint(url).is_err(), "{url}");
        }
    }
}
