//! 扩展开发技能包（Skills）：把客户端内置的 `x-hub-extension` skill 一键安装到
//! 本机 AI 编码助手（Claude Code / DSH / Codex 等）的 skills 目录。
//!
//! 设计要点：
//! - **内置源 = 仓库 `skills/x-hub-extension/`**，由 `build.rs` 生成清单后烘焙进二进制
//!   （见 `OUT_DIR/skill_files.rs`）——单一真相源、随客户端版本走，不额外分发文件；
//! - **「已安装 / 可更新」不落宿主状态文件**：靠目标目录里的 `.xhub-skill.json` 标记
//!   （记录安装时的内容哈希）与本二进制的内容哈希比对判定，用户手动拷进来的副本也能识别；
//! - 自动探测的已知 skills 根**只在已存在时列出**（不代用户新建目录），另有自定义目录入口。
//!
//! 设置页入口：设置 → 扩展 → Skills（`src/components/settings/SkillsSection.vue`）。

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

// build.rs 生成的内置包清单（相对路径 + 文件字节）
include!(concat!(env!("OUT_DIR"), "/skill_files.rs"));

/// 技能 id（与 SKILL.md front matter 的 name 一致）
pub const SKILL_ID: &str = "x-hub-extension";
/// 安装到目标 skills 根下的子目录名
const SKILL_DIR_NAME: &str = "x-hub-extension";
/// 安装标记文件：记录来源 / 版本 / 内容哈希，用于「已安装 / 可更新」判定
const MARKER_FILE: &str = ".xhub-skill.json";

/// 技能包元信息（前端展示 + 判定基准）
#[derive(Debug, Clone, Serialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 安装到目标根下的子目录名
    pub dir_name: String,
    /// 文件数
    pub file_count: usize,
    /// 原始字节数
    pub size: u64,
    /// 内容哈希（sha256 前 16 位十六进制；展示与比对）
    pub hash: String,
    /// 随客户端版本（写入标记，界面显示「随客户端 vX.Y.Z」）
    pub app_version: String,
}

/// 一个安装目标（= 某助手的 skills 根目录）
#[derive(Debug, Clone, Serialize)]
pub struct SkillTarget {
    /// 展示名（Claude Code / DSH / Codex / 自定义目录）
    pub label: String,
    /// skills 根目录绝对路径
    pub path: String,
    /// auto = 自动探测的已知助手目录；custom = 用户添加的自定义目录
    pub kind: String,
    /// 目标下已存在本技能目录
    pub installed: bool,
    /// 已安装且内容与内置一致（无需更新）
    pub up_to_date: bool,
    /// 目录存在但不是本客户端装的（无标记文件）：覆盖前需用户二次确认
    pub foreign: bool,
    /// 标记里记录的安装版本（无标记时为 null）
    pub installed_version: Option<String>,
}

/// 技能总览（前端一次拉取）
#[derive(Debug, Clone, Serialize)]
pub struct SkillOverview {
    pub skill: SkillInfo,
    pub targets: Vec<SkillTarget>,
}

// ---------------- 内置包内容 ----------------

/// 全包内容哈希（进程内只算一次）
fn bundle_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        let mut files: Vec<&(&'static str, &'static [u8])> = EMBEDDED_SKILL_FILES.iter().collect();
        files.sort_by(|a, b| a.0.cmp(b.0));
        let mut hasher = Sha256::new();
        for (rel, bytes) in files {
            hasher.update(rel.as_bytes());
            hasher.update([0u8]);
            hasher.update(bytes);
        }
        let full = format!("{:x}", hasher.finalize());
        full[..16].to_string()
    })
}

fn bundle_size() -> u64 {
    EMBEDDED_SKILL_FILES.iter().map(|(_, b)| b.len() as u64).sum()
}

/// 从 SKILL.md 的 YAML front matter 取字段（name / description）
fn front_matter_field(content: &str, key: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let prefix = format!("{key}:");
    for line in lines {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(rest) = t.strip_prefix(prefix.as_str()) {
            let v = rest
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .trim()
                .to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

fn skill_info() -> SkillInfo {
    let skill_md = EMBEDDED_SKILL_FILES
        .iter()
        .find(|(rel, _)| *rel == "SKILL.md")
        .and_then(|(_, b)| std::str::from_utf8(b).ok())
        .unwrap_or("");
    SkillInfo {
        id: SKILL_ID.to_string(),
        name: front_matter_field(skill_md, "name").unwrap_or_else(|| SKILL_ID.to_string()),
        description: front_matter_field(skill_md, "description").unwrap_or_default(),
        dir_name: SKILL_DIR_NAME.to_string(),
        file_count: EMBEDDED_SKILL_FILES.len(),
        size: bundle_size(),
        hash: bundle_hash().to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

// ---------------- 目标探测与状态 ----------------

/// 自动探测的已知助手 skills 根（仅存在的会被列出）
fn auto_roots() -> Vec<(&'static str, PathBuf)> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut roots = vec![
        ("Claude Code", home.join(".claude").join("skills")),
        ("DSH / 通用 Agent", home.join(".agents").join("skills")),
        ("Codex", home.join(".codex").join("skills")),
        ("ZCode", home.join(".zcode").join("skills")),
        ("WorkBuddy", home.join(".workbuddy").join("skills")),
    ];
    // 豆包（桌面版）的用户技能目录在其 Chromium 配置树内：.skills 是内置技能，
    // .user_skills 才是用户层（安装到内置目录会被升级覆盖）
    if let Some(local) = dirs::data_local_dir() {
        roots.push((
            "豆包",
            local
                .join("Doubao")
                .join("User Data")
                .join("Default")
                .join(".doubao")
                .join("agent_mode")
                .join("workspace")
                .join(".user_skills"),
        ));
    }
    roots
}

/// 规范路径键：canonicalize 成功用真身（解析 junction / symlink），失败回退原路径。
/// 「通用目录」判定基准——两个根指向同一物理目录时合并为一条，不重复列行。
fn canon_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// 汇总单个目标的状态
fn target_status(label: &str, root: &Path, kind: &str) -> SkillTarget {
    let dir = root.join(SKILL_DIR_NAME);
    let marker = dir.join(MARKER_FILE);
    let installed = dir.is_dir();
    let mut installed_version = None;
    let mut up_to_date = false;
    if marker.is_file() {
        if let Ok(text) = std::fs::read_to_string(&marker) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if v.get("hash").and_then(|h| h.as_str()) == Some(bundle_hash()) {
                    up_to_date = true;
                }
                installed_version = v
                    .get("appVersion")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
            }
        }
    }
    SkillTarget {
        label: label.to_string(),
        path: root.to_string_lossy().into_owned(),
        kind: kind.to_string(),
        installed,
        up_to_date,
        // 目录在但不是本客户端装的（没有标记文件）
        foreign: installed && !marker.is_file(),
        installed_version,
    }
}

fn overview() -> SkillOverview {
    let cfg = crate::config::load();
    let mut targets: Vec<SkillTarget> = Vec::new();
    // 规范路径 → targets 下标：同物理目录只列一行，后来者的名字并进 label
    //（如 ~/.zcode/skills 是 ~/.agents/skills 的 junction → 显示「DSH / 通用 Agent、ZCode」）
    let mut by_canon: Vec<(PathBuf, usize)> = Vec::new();

    for (label, root) in auto_roots() {
        if !root.is_dir() {
            continue;
        }
        let key = canon_key(&root);
        if let Some((_, idx)) = by_canon.iter().find(|(k, _)| *k == key) {
            targets[*idx].label.push_str(&format!("、{label}"));
            continue;
        }
        by_canon.push((key, targets.len()));
        targets.push(target_status(label, &root, "auto"));
    }
    for raw in &cfg.skill_roots {
        let root = PathBuf::from(raw);
        if !root.is_dir() {
            continue;
        }
        let key = canon_key(&root);
        if let Some((_, idx)) = by_canon.iter().find(|(k, _)| *k == key) {
            targets[*idx].label.push_str("、自定义目录");
            continue;
        }
        by_canon.push((key, targets.len()));
        targets.push(target_status("自定义目录", &root, "custom"));
    }

    SkillOverview {
        skill: skill_info(),
        targets,
    }
}

// ---------------- 安装 / 卸载 ----------------

/// 把内置包逐文件写到目标目录（相对路径逐段建目录）
fn write_bundle(dir: &Path) -> Result<(), String> {
    for (rel, bytes) in EMBEDDED_SKILL_FILES {
        let dst = dir.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("IO_ERROR: 建目录失败：{e}"))?;
        }
        std::fs::write(&dst, bytes).map_err(|e| format!("IO_ERROR: 写入 {rel} 失败：{e}"))?;
    }
    Ok(())
}

/// 写安装标记（内容哈希 = 可更新判定基准）
fn write_marker(dir: &Path) -> Result<(), String> {
    let marker = serde_json::json!({
        "id": SKILL_ID,
        "hash": bundle_hash(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "installedAt": chrono::Utc::now().to_rfc3339(),
    });
    let text = serde_json::to_vec_pretty(&marker).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(MARKER_FILE), text).map_err(|e| format!("IO_ERROR: 写标记失败：{e}"))
}

/// 自定义目录登记进配置（下次仍能列出；自动探测的根不登记——按规范路径比，
/// 自定义目录即使写法不同、只要是同一个物理目录就不重复登记）
fn register_root(root: &Path) {
    let key = canon_key(root);
    for (_, p) in auto_roots() {
        if p.is_dir() && canon_key(&p) == key {
            return;
        }
    }
    let _guard = crate::config::lock();
    let mut cfg = crate::config::load();
    if !cfg.skill_roots.iter().any(|d| Path::new(d) == root) {
        cfg.skill_roots.push(root.to_string_lossy().into_owned());
        if let Err(e) = crate::config::save(&cfg) {
            log::warn!("自定义 skills 目录登记失败：{e}");
        }
    }
}

/// 读取技能包总览（内置元信息 + 所有已存在目标的状态）
#[tauri::command]
pub fn get_skill_overview() -> Result<SkillOverview, String> {
    Ok(overview())
}

/// 安装 / 更新到指定 skills 根（写入 `<path>/x-hub-extension`）。
/// 目标已存在同名目录时先返回 `CONFLICT`，前端二次确认后带 `force = true` 覆盖（内容原子替换）。
#[tauri::command]
pub fn install_skill(path: String, force: bool) -> Result<SkillOverview, String> {
    let root = PathBuf::from(path.trim());
    if !root.is_dir() {
        return Err(format!("INVALID_ARGUMENT: 目录不存在：{}", root.display()));
    }
    let target = root.join(SKILL_DIR_NAME);
    let marker = target.join(MARKER_FILE);
    if target.exists() && !force {
        return Err(if marker.is_file() {
            "CONFLICT: 已安装，需确认后覆盖更新".to_string()
        } else {
            "CONFLICT: 目标已存在同名目录且不是本客户端安装的，需确认后覆盖".to_string()
        });
    }

    // 先写临时目录，再原子替换：中途失败不会留下半截安装
    let tmp = root.join(format!(".{SKILL_DIR_NAME}.new"));
    let old = root.join(format!(".{SKILL_DIR_NAME}.old"));
    let _ = std::fs::remove_dir_all(&tmp);
    write_bundle(&tmp)?;
    write_marker(&tmp)?;

    if target.exists() {
        let _ = std::fs::remove_dir_all(&old);
        std::fs::rename(&target, &old).map_err(|e| format!("IO_ERROR: 备份原目录失败：{e}"))?;
    }
    match std::fs::rename(&tmp, &target) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&old);
        }
        Err(e) => {
            // 回滚：把备份还原回去，清掉临时目录
            if old.exists() {
                let _ = std::fs::rename(&old, &target);
            }
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(format!("IO_ERROR: 写入失败：{e}"));
        }
    }

    register_root(&root);
    log::info!("技能已安装：{} -> {}", SKILL_ID, target.display());
    Ok(overview())
}

/// 从指定 skills 根卸载（仅删本技能目录，不动同根下其它 skill）
#[tauri::command]
pub fn uninstall_skill(path: String) -> Result<SkillOverview, String> {
    let target = PathBuf::from(path.trim()).join(SKILL_DIR_NAME);
    if !target.is_dir() {
        return Err("NOT_FOUND: 该目录下没有已安装的技能".to_string());
    }
    std::fs::remove_dir_all(&target).map_err(|e| format!("IO_ERROR: 卸载失败：{e}"))?;
    log::info!("技能已卸载：{} <- {}", SKILL_ID, target.display());
    Ok(overview())
}

/// 从设置页列表移除一个自定义目录（只解除登记，不动磁盘文件）
#[tauri::command]
pub fn remove_skill_root(path: String) -> Result<SkillOverview, String> {
    let root = PathBuf::from(path.trim());
    let _guard = crate::config::lock();
    let mut cfg = crate::config::load();
    let before = cfg.skill_roots.len();
    cfg.skill_roots.retain(|d| Path::new(d) != root.as_path());
    if cfg.skill_roots.len() != before {
        crate::config::save(&cfg)?;
    }
    Ok(overview())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_contains_skill_md_and_hash_is_stable() {
        assert!(EMBEDDED_SKILL_FILES.iter().any(|(rel, _)| *rel == "SKILL.md"));
        assert!(EMBEDDED_SKILL_FILES.iter().any(|(rel, _)| *rel == "xhub.d.ts"));
        assert_eq!(bundle_hash().len(), 16);
        assert_eq!(bundle_hash(), bundle_hash());
        assert!(bundle_size() > 0);
    }

    #[test]
    fn skill_info_reads_front_matter() {
        let info = skill_info();
        assert_eq!(info.name, "x-hub-extension");
        assert!(!info.description.is_empty());
        assert_eq!(info.dir_name, SKILL_DIR_NAME);
        assert_eq!(info.file_count, EMBEDDED_SKILL_FILES.len());
    }

    #[test]
    fn install_then_status_is_up_to_date() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(SKILL_DIR_NAME);
        write_bundle(&dir).unwrap();
        write_marker(&dir).unwrap();

        let st = target_status("Test", tmp.path(), "custom");
        assert!(st.installed);
        assert!(st.up_to_date);
        assert!(!st.foreign);
        assert_eq!(
            st.installed_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );

        // 去掉标记文件 → 视为「非本客户端安装」的外来目录
        std::fs::remove_file(dir.join(MARKER_FILE)).unwrap();
        let st = target_status("Test", tmp.path(), "custom");
        assert!(st.installed && st.foreign && !st.up_to_date);
    }
}

