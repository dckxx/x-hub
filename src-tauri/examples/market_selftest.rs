//! 市场模块离线自检（`cargo run --example market_selftest`）。
//!
//! 背景：当前开发机的 cargo test test-harness 构建产物存在 0xc0000139 环境问题
//! （改动前的旧 test 二进制同样失败，与本模块无关），本 example 用独立二进制
//! **不经过 test harness** 验证市场关键逻辑，等价于客户端侧端到端检查：
//!   1. Ed25519 验签已知向量（signing::verify_detached）；
//!   2. 以客户端视角读取 `dist/market/registry.json + .sig` 真实发布产物并验签；
//!   3. 清单 v2 解析 + v1 旧格式兼容解析（复用 app_lib::market::MarketExtension）；
//!   4. 对照 zip 包 sha256 与清单条目一致。
//! 用法：先运行 `scripts/publish-extension.ps1` 生成产物，再 `cargo run --example market_selftest`。

use app_lib::market::{replace_extension_dir, version_cmp, MarketExtension};
use app_lib::signing::verify_detached;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::PathBuf;

#[derive(serde::Deserialize, Debug)]
struct RegistryFile {
    #[serde(rename = "schemaVersion", default)]
    schema_version: u32,
    #[serde(rename = "updatedAt", default)]
    updated_at: String,
    #[serde(default)]
    extensions: Vec<MarketExtension>,
}

fn main() {
    let mut failed = 0u32;

    // 1) 已知测试向量（与 signing.rs 单测同一对）
    const DATA: &str = "x-hub market registry test vector v1";
    const SIG: &str = "Z91eGY0mPfeEChpfyvgEhSXo+BoCTODapsDObnOO9gc74OjEFRXRMVoZXn1S7XDV1iLVKZ5xPZtG5bpXz0zZDQ==";
    match verify_detached(DATA.as_bytes(), SIG) {
        Ok(()) => println!("[PASS] 已知向量验签通过"),
        Err(e) => {
            println!("[FAIL] 已知向量验签失败: {e}");
            failed += 1;
        }
    }

    // 2) 真实发布产物（客户端视角整链路）
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist-market");
    let reg_path = root.join("registry.json");
    let sig_path = root.join("registry.json.sig");
    if reg_path.is_file() && sig_path.is_file() {
        let content = std::fs::read(&reg_path).expect("读取 registry.json");
        let sig = std::fs::read_to_string(&sig_path).expect("读取 registry.json.sig");
        match verify_detached(&content, sig.trim()) {
            Ok(()) => println!("[PASS] dist-market/registry.json 验签通过（客户端可接受）"),
            Err(e) => {
                println!("[FAIL] 发布产物验签失败: {e}");
                failed += 1;
            }
        }
        let reg: RegistryFile = match serde_json::from_slice(&content) {
            Ok(r) => r,
            Err(e) => {
                println!("[FAIL] 清单解析失败: {e}");
                std::process::exit(1);
            }
        };
        println!(
            "[INFO] schemaVersion={} updatedAt={} extensions={}",
            reg.schema_version,
            reg.updated_at,
            reg.extensions.len()
        );
        for ext in &reg.extensions {
            // downloadUrl 去掉 CDN 前缀后即为市场根下相对路径
            let rel = ext
                .download_url
                .split("extensions/")
                .nth(1)
                .unwrap_or(&ext.download_url);
            let zip_path = root.join(rel);
            if !zip_path.is_file() {
                println!("[FAIL] zip 包不存在: {}", zip_path.display());
                failed += 1;
                continue;
            }
            let bytes = std::fs::read(&zip_path).expect("读取 zip");
            let actual: String = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if actual.eq_ignore_ascii_case(&ext.sha256) {
                println!("[PASS] {} v{} zip sha256 与清单一致", ext.id, ext.version);
            } else {
                println!("[FAIL] {} sha256 不符: 清单={} 实际={}", ext.id, ext.sha256, actual);
                failed += 1;
            }
        }
    } else {
        println!(
            "[SKIP] 未找到发布产物 {}（先运行 scripts/publish-extension.ps1 再执行本程序）",
            root.display()
        );
    }

    // 3) v1 旧格式兼容解析
    let legacy = r#"{"extensions":[{"id":"com.x-hub.legacy","name":"老扩展","version":"0.1.0","description":"legacy","runtime":"web","author":"x","downloadUrl":"https://example.com/a.zip"}]}"#;
    match serde_json::from_str::<RegistryFile>(legacy) {
        Ok(r) if r.schema_version == 0 && r.extensions.len() == 1 && r.extensions[0].sha256.is_empty() => {
            println!("[PASS] v1 旧格式兼容解析")
        }
        other => {
            println!("[FAIL] v1 兼容解析异常: {other:?}");
            failed += 1;
        }
    }

    // 4) 版本比较（semver + 非 semver 回退）
    let vc = [
        ("1.2.0", "1.1.9", Ordering::Greater),
        ("0.1.0", "0.1.0", Ordering::Equal),
        ("0.1.0", "0.2.0", Ordering::Less),
        ("0.10.0", "0.9.9", Ordering::Greater),
        ("1.2", "1.10", Ordering::Less),
        ("2", "1.9", Ordering::Greater),
    ];
    if vc.iter().all(|(a, b, want)| version_cmp(a, b) == *want) {
        println!("[PASS] 版本比较 semver/回退 6 例");
    } else {
        println!("[FAIL] 版本比较出现不符");
        failed += 1;
    }

    // 5) 扩展目录替换（升级核心：备份旧 -> 新就位 -> 保留用户点文件）
    let tmp = std::env::temp_dir().join(format!("xhub-update-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dest = tmp.join("com.x-hub.hello-web");
    let content = tmp.join("content");
    let backup = tmp.join("backup");
    // 旧目录：manifest v0.1.0 + 用户点文件 .config.json / .storage.json
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(
        dest.join("manifest.json"),
        r#"{"id":"com.x-hub.hello-web","version":"0.1.0"}"#,
    )
    .unwrap();
    std::fs::write(dest.join(".config.json"), r#"{"theme":"dark"}"#).unwrap();
    std::fs::write(dest.join(".storage.json"), r#"{"n":1}"#).unwrap();
    // 新内容：manifest v0.2.0 + 副本
    std::fs::create_dir_all(&content).unwrap();
    std::fs::write(
        content.join("manifest.json"),
        r#"{"id":"com.x-hub.hello-web","version":"0.2.0"}"#,
    )
    .unwrap();
    std::fs::write(content.join("index.js"), "console.log(1)").unwrap();

    match replace_extension_dir(&dest, &content, &backup) {
        Ok(()) => {
            let new_ok = dest.join("manifest.json").is_file()
                && dest.join("index.js").is_file()
                && dest.join(".config.json").is_file()
                && dest.join(".storage.json").is_file()
                && !backup.exists()
                && !content.exists();
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dest.join("manifest.json")).unwrap())
                    .unwrap();
            let new_version = v["version"].as_str().unwrap_or("");
            if new_ok && new_version == "0.2.0" {
                println!("[PASS] 扩展目录替换：旧备份/新就位/点文件保留");
            } else {
                println!("[FAIL] 替换结果异常（new_ok={new_ok} new_version={new_version}）");
                failed += 1;
            }
        }
        Err(e) => {
            println!("[FAIL] 目录替换报错: {e}");
            failed += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);

    if failed > 0 {
        println!("\n市场模块自检：{failed} 项失败");
        std::process::exit(1);
    }
    println!("\n市场模块自检全部通过");
}