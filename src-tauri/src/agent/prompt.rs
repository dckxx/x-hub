//! System prompt 组装（计划 §4 已定，#t44）：四段——身份边界 / 当前时间 / 行为准则 / 关于我。
//!
//! 当前时间**注入而不是做成工具**：做成工具模型每轮都得先花一次调用问"今天几号"，
//! 费额度又慢；时间属于「每轮都需要的背景信息」。故首批工具表里没有日期工具。

use chrono::{Datelike, Timelike};

/// 组装 system prompt。`now` 用本地时间；`about_me` 为设置「AI 助手 → 关于我」的
/// 用户自填档案（首期只读注入，模型不写回；空/None 时整段省略）。
pub fn build_system_prompt(now: chrono::DateTime<chrono::Local>, about_me: Option<&str>) -> String {
    let mut s = String::new();

    // 1. 身份与边界
    s.push_str(
        "你是 x-hub 个人效率工作台的内置助手。你可以通过工具直接操作本机的笔记、待办、提示词、\
速达资源、便签、剪贴板与倒计时。已安装的 service 扩展还会通过 extension_tool_list 声明自己的\
数据与功能（比如某个扩展的记录查询），需要时先列出再调用，不要凭空臆造扩展能力。你没有的能力\
（联网搜索、写代码、操作文件系统、跨会话记忆）要如实说明，不要假装做过。\n\n",
    );

    // 2. 当前时间（动态注入）
    s.push_str(&format!("现在是 {}。\n\n", time_text(now)));

    // 3. 行为准则
    s.push_str(
        "行为准则：\n\
1. 需要事实（比如「我有什么待办」「那篇笔记写了什么」）时必须用工具查证，不要凭记忆编造。\n\
2. 执行写操作前，用一句话向用户说明你将要做什么。\n\
3. 写操作必须**实际调用对应工具**才算完成：没有发出工具调用就宣称「已记录 / 已创建 / 已完成」\
是严格禁止的——没调用工具就是没做成，用户那边什么都不会出现。工具执行报错、找不到对应工具、\
权限不足时，如实说明失败原因，不要假装成功。\n\
4. 一次只做用户要求的事，不擅自扩大范围。\n\
5. 跟随用户的语言回复。\n\
6. 按名称匹配的参数（文件夹、标签、小类等）如果精确匹配失败，把最接近的候选列表抛回给用户\
确认，不要猜、也不要静默落到默认位置。\n",
    );

    // 4. 用户档案（可选）
    if let Some(text) = about_me.map(str::trim).filter(|t| !t.is_empty()) {
        s.push_str(&format!(
            "\n用户档案（用户自填的背景信息，供你参考）：\n{text}\n"
        ));
    }
    s
}

/// 时间行：`2026-08-29 星期六 14:32`
fn time_text(now: chrono::DateTime<chrono::Local>) -> String {
    let weekday = match now.weekday() {
        chrono::Weekday::Mon => "星期一",
        chrono::Weekday::Tue => "星期二",
        chrono::Weekday::Wed => "星期三",
        chrono::Weekday::Thu => "星期四",
        chrono::Weekday::Fri => "星期五",
        chrono::Weekday::Sat => "星期六",
        chrono::Weekday::Sun => "星期日",
    };
    format!(
        "{:04}-{:02}-{:02} {} {:02}:{:02}",
        now.year(),
        now.month(),
        now.day(),
        weekday,
        now.hour(),
        now.minute()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn prompt_has_four_sections_and_injected_time() {
        let now = chrono::Local.with_ymd_and_hms(2026, 8, 29, 14, 32, 0).unwrap();
        let p = build_system_prompt(now, None);
        assert!(p.contains("x-hub 个人效率工作台"));
        assert!(p.contains("你没有的能力"));
        assert!(p.contains("现在是 2026-08-29 星期六 14:32"));
        assert!(p.contains("必须用工具查证"));
        // 反谎报铁律：没有工具调用就宣称完成是严格禁止的（平台模型曾对「记个待办」
        // 不调 create_todo 直接编「已帮你记录 ✅」——库里有案底）
        assert!(p.contains("没有发出工具调用就宣称"));
        assert!(p.contains("候选列表"));
        // 没填「关于我」时不出现档案段
        assert!(!p.contains("用户档案"));
    }

    #[test]
    fn about_me_appended_when_present() {
        let now = chrono::Local.with_ymd_and_hms(2026, 1, 5, 8, 5, 0).unwrap();
        let p = build_system_prompt(now, Some("我是做后端的，回复请简洁"));
        assert!(p.contains("用户档案"));
        assert!(p.contains("我是做后端的，回复请简洁"));
        // 空白档案视同未填
        let p2 = build_system_prompt(now, Some("   "));
        assert!(!p2.contains("用户档案"));
    }
}
