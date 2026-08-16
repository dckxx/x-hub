pub mod countdown;
pub mod chat;
pub mod detached_sticky;
pub mod note;
pub mod resource;
pub mod snippet;
pub mod sticky;
pub mod tag;
pub mod todo;

/// 生成纳秒精度的 UTC 时间戳，用于保证排序唯一性
pub fn now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string()
}
