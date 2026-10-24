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
    //    第 1 条是「要不要查」的前置闸门，必须有：旧版只有「需要事实时必须查证」这一条正向
    //    强制、没有反向门槛，flash 级模型会泛化成「任何消息都先查一圈再说」（用户反馈：
    //    随便说点什么都去翻笔记/待办）。闸门只约束**读探查**；写操作仍归第 5 条铁律管，
    //    第 1 条末句把两者切开，防止模型从「事事都查」摆到「该查也不查、直接编」。
    s.push_str(
        "行为准则：\n\
1. 先判断要不要查，再决定查什么。判断标准是「用户这句话的答案是否依赖本机数据」：\n\
   - 依赖就查：「我有什么待办」「那篇笔记写了什么」「帮我把昨天记的那条改一下」这类必须调用\
对应工具，查到什么就说什么。\n\
   - 不依赖就直接回答，**不要**为了「顺便看一眼」去调用 list_todos、search_notes、\
global_search 等读工具：闲聊问候、通用知识问答、解释概念、写作润色翻译、出主意都属于这一类。\n\
   - 拿不准时先直接回答或反问一句，不要靠先调一轮工具来试探。\n\
   本条只管「读探查」，不影响写操作：用户让你记待办、建笔记、存提示词时，对应的写工具照旧必须调用。\n\
2. 查一次就够，没命中不要搜刮：关键词没搜到时最多换一个更宽泛的词重试一次；仍然没有就如实说\
「没找到」，不要接着换工具把整个笔记库或待办列表拉一遍。\n\
3. 既然判定要查，就必须真的查：需要事实（比如「我有什么待办」「那篇笔记写了什么」）时用工具\
查证，不要凭记忆编造。\n\
4. 执行写操作前，用一句话向用户说明你将要做什么。\n\
5. 写操作必须**实际调用对应工具**才算完成：没有发出工具调用就宣称「已记录 / 已创建 / 已完成」\
是严格禁止的——没调用工具就是没做成，用户那边什么都不会出现。工具执行报错、找不到对应工具、\
权限不足时，如实说明失败原因，不要假装成功。\n\
6. 一次只做用户要求的事，不擅自扩大范围。\n\
7. 跟随用户的语言回复。\n\
8. 按名称匹配的参数（文件夹、标签、小类等）如果精确匹配失败，把最接近的候选列表抛回给用户\
确认，不要猜、也不要静默落到默认位置。\n\
9. 用户口中的数字可能只是条目的标题/名称（比如一篇名叫「20261023」的笔记）而不是 id：先把数字\
当关键词用搜索工具确认，拿到返回的 [id] 再操作，不要直接把数字当 id 传参。工具报「同名多篇」\
时把候选列给用户确认，报「不存在」时如实转述，不要猜。\n",
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
        assert!(p.contains("不要凭记忆编造"));
        // 反谎报铁律：没有工具调用就宣称完成是严格禁止的（平台模型曾对「记个待办」
        // 不调 create_todo 直接编「已帮你记录 ✅」——库里有案底）
        assert!(p.contains("没有发出工具调用就宣称"));
        assert!(p.contains("候选列表"));
        // 没填「关于我」时不出现档案段
        assert!(!p.contains("用户档案"));
    }

    /// 「要不要查」的前置闸门（2026-10-21 修「随便说什么都去翻笔记/待办」）：
    /// 必须同时存在三样东西，缺一即回归——
    /// ① 判定标准是「答案是否依赖本机数据」；② 明确禁止无关消息顺手调读工具；
    /// ③ 闸门只管读探查、写工具照旧必须调用（防从「事事都查」摆到「该查也不查」）
    #[test]
    fn prompt_gates_read_tools_but_keeps_write_rules() {
        let now = chrono::Local.with_ymd_and_hms(2026, 10, 21, 9, 0, 0).unwrap();
        let p = build_system_prompt(now, None);
        assert!(p.contains("答案是否依赖本机数据"), "缺少判定标准：{p}");
        assert!(p.contains("不要") && p.contains("为了「顺便看一眼」"), "缺少反向禁止：{p}");
        assert!(p.contains("闲聊问候"), "缺少「无关消息」的正例说明：{p}");
        assert!(
            p.contains("本条只管「读探查」") && p.contains("写工具照旧必须调用"),
            "闸门必须显式排除写操作，否则会吓退该调的写工具：{p}"
        );
        // 搜刮收敛：一次换词重试，再无结果如实说没找到
        assert!(p.contains("没命中不要搜刮"), "缺少无命中收敛规则：{p}");
        assert!(p.contains("最多换一个更宽泛的词重试一次"), "缺少重试上限：{p}");
        // 反谎报铁律与闸门并存（两条都在，谁也没被谁挤掉）
        assert!(p.contains("没有发出工具调用就宣称"));
        assert!(p.contains("不要凭记忆编造"));
    }

    /// Rust 字符串用 `\` 续行时**会吞掉下一行的前导空白**——断点选在符号中间就会把
    /// 两个词粘成一个（实测踩过：`search_notes、` 断在行尾后 `global_search` 接续，
    /// 渲染成 `search_notes、global_search` 是对的，但断在 `/` 后就会粘成 `notes /global_search`）。
    /// 这里直接在渲染结果上断言，别靠读源码看。
    #[test]
    fn prompt_line_continuations_do_not_glue_words() {
        let now = chrono::Local.with_ymd_and_hms(2026, 10, 21, 9, 0, 0).unwrap();
        let p = build_system_prompt(now, None);
        // 断点落在斜杠后面会渲染出 " /global_search" 这种粘连
        assert!(!p.contains("/global_search"), "续行把工具名粘在一起了：{p}");
        // 三个工具名必须各自独立出现
        for name in ["list_todos", "search_notes", "global_search"] {
            assert!(p.contains(name), "缺少工具名 {name}：{p}");
        }
    }

    #[test]
    fn about_me_appended_when_present() {        let now = chrono::Local.with_ymd_and_hms(2026, 1, 5, 8, 5, 0).unwrap();
        let p = build_system_prompt(now, Some("我是做后端的，回复请简洁"));
        assert!(p.contains("用户档案"));
        assert!(p.contains("我是做后端的，回复请简洁"));
        // 空白档案视同未填
        let p2 = build_system_prompt(now, Some("   "));
        assert!(!p2.contains("用户档案"));
    }
}
