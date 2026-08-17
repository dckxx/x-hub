/// 内置更新日志（版本历史），随版本打包进二进制，运行时零网络请求。
///
/// 单一数据源：仓库根目录 `RELEASE_NOTES.md`，采用累积式结构——
/// 每次发版在文件顶部新增一节 `# vX.Y.Z 发布说明`，历史版本依次排在其后。
pub const RELEASE_NOTES: &str = include_str!("../../RELEASE_NOTES.md");

/// 按 `# vX.Y.Z` 一级标题把完整版本历史切分为若干版本段（最新在前），
/// 每段保留其标题与全部正文。
pub fn version_sections() -> Vec<String> {
    let mut sections: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in RELEASE_NOTES.lines() {
        // 遇到新的一级标题且当前段非空，则先归档当前段
        if line.starts_with("# ") && !current.is_empty() {
            sections.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        sections.push(current);
    }
    sections
}

/// 最新一段版本说明（升级后「What's New」弹窗使用）
pub fn latest_section() -> String {
    version_sections()
        .into_iter()
        .next()
        .unwrap_or_else(|| RELEASE_NOTES.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_sections_split_on_h1() {
        let sections = version_sections();
        assert!(!sections.is_empty());
        // 首段应为一号标题
        assert!(sections[0].starts_with("# v"));
    }
}
