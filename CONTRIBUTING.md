# 贡献指南

感谢对 x-hub 的关注。项目目前只维护最新版本，不设旧版本维护分支。

## 开发环境

- Node.js 20
- Rust stable（含 Tauri 2 依赖）
- Windows（CI 在 windows-latest 上验证）

## 本地验证

```bash
npm ci
npm run build        # vue-tsc 类型检查 + vite 构建
cargo check --manifest-path src-tauri/Cargo.toml
```

与 CI（`.github/workflows/ci.yml` 的 `check` 作业）保持一致，PR 必须通过 CI。

## 提 PR

1. fork 后从 `master` 切短命分支，合并后删除。
2. PR 目标分支 `master`，描述写清动机、改动点、验证方式。
3. 大改动请先开 issue 讨论再动手。
4. 合并方式为 squash，保持历史线性。

## 支持策略

只维护最新版本；问题与功能请求请基于最新版提出。

## 许可

MIT（见 `LICENSE`）。
