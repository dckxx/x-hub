//! 云同步（docs/sync-protocol.md）。
//!
//! 分层：crypto（P0）→ repo（outbox/墓碑/meta/guard）→ map（业务行 ⇄ 明文载荷）
//! → engine（拉-合-推）→ api / pairing / commands。跨端一致性锚点：
//! `test_vectors.json`（由小程序仓 scripts/gen-sync-vectors.mjs 生成）。

pub mod api;
pub mod commands;
pub mod crypto;
pub mod engine;
pub mod map;
pub mod pairing;
pub mod repo;
