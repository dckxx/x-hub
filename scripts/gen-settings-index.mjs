/**
 * 生成设置项索引：src/components/settingsIndex.generated.ts
 *
 * 为什么需要它：设置页改成「只挂载当前大类」之后，其它大类的设置项**不在 DOM 里**，
 * 搜索框就不能靠遍历页面找。索引必须独立存在，且不能手写（手写必然随实现漂移）。
 *
 * 所以索引从 SettingsView.vue 的模板里按分区提取 `setting-name` 文本：
 *   - 纯文本条目 → 收录
 *   - 含 {{ }} 插值的（如账号名、扩展名）→ 跳过（不是稳定的搜索目标）
 *
 * 用法：
 *   node scripts/gen-settings-index.mjs          写入文件（package.json 的 prebuild 会自动跑）
 *   node scripts/gen-settings-index.mjs --check   只校验、不写（不一致时退出码 1）
 */
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { splitGuideDocument } from "../src/utils/guideToc.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(root, "src/components/settingsIndex.generated.ts");

// 分区已经按大类拆进 src/components/settings/*.vue（见 SettingsView.vue 头部说明），
// 所以索引要扫外壳 + 所有面板，不能只扫外壳。
const SETTINGS_DIR = join(root, "src/components/settings");
const FILES = [
  join(root, "src/components/SettingsView.vue"),
  ...readdirSync(SETTINGS_DIR)
    .filter((f) => f.endsWith(".vue"))
    .sort()
    .map((f) => join(SETTINGS_DIR, f)),
];

// 刻意隐藏、但源码里保留待恢复的设置项：<!-- settings-index:skip-start --> … <!-- settings-index:skip-end -->
// 提取前整段剔除 —— 否则搜索会命中一个界面上根本不存在的条目（点进去只能落到分区顶部）。
const SKIP_RE = /<!--\s*settings-index:skip-start\s*-->[\s\S]*?<!--\s*settings-index:skip-end\s*-->/g;
const SKIP_START = /settings-index:skip-start/g;
const SKIP_END = /settings-index:skip-end/g;

// 按分区起始标签切块
const secRe = /<section[^>]*id="sv-sec-([a-z]+)"[^>]*>/g;
const seen = new Set();
const items = [];
let sectionCount = 0;
// 设置项标题有两套 class：通用项用 setting-name，外观区的主题/壁纸那几项用 theme-label
// （漏掉后者就搜不到「主题模式 / 强调色 / 应用壁纸」这类最常改的项）
const TITLE_RES = [
  /<span class="setting-name">([^<>{}]+)<\/span>/g,
  /<span class="theme-label">([^<>{}]+)<\/span>/g,
  // 分区内的小组标题（如扩展区的「我的扩展」、剪贴板的「保留策略」）：也是有意义的定位锚点
  /<h4 class="sv-subtitle">([^<>{}]+)<\/h4>/g,
];

for (const file of FILES) {
  const raw = readFileSync(file, "utf8");
  const starts = [...raw.matchAll(SKIP_START)].length;
  const ends = [...raw.matchAll(SKIP_END)].length;
  if (starts !== ends) {
    console.error(
      `[settings-index] ${file} 的 skip 标记不成对（start ${starts} / end ${ends}）：` +
        `隐藏区必须用 <!-- settings-index:skip-start --> 与 <!-- settings-index:skip-end --> 成对包住`,
    );
    process.exit(1);
  }
  const src = raw.replace(SKIP_RE, "");
  const marks = [...src.matchAll(secRe)].map((m) => ({
    id: m[1],
    start: m.index,
    end: m.index + m[0].length,
  }));
  sectionCount += marks.length;
  for (let i = 0; i < marks.length; i++) {
    const body = src.slice(marks[i].end, i + 1 < marks.length ? marks[i + 1].start : src.length);
    const found = [];
    for (const re of TITLE_RES) {
      for (const m of body.matchAll(re)) found.push({ at: m.index ?? 0, title: m[1].trim() });
    }
    // 按模板里的出现顺序排（两套 class 混在同一分区时要保持视觉顺序）
    found.sort((a, b) => a.at - b.at);
    for (const f of found) {
      if (!f.title) continue;
      const key = `${marks[i].id}:${f.title}`;
      if (seen.has(key)) continue;
      seen.add(key);
      items.push({ section: marks[i].id, title: f.title });
    }
  }
}
if (sectionCount === 0) {
  console.error("[settings-index] 没找到任何 sv-sec-* 分区：外壳与 settings/*.vue 结构都变了吗？");
  process.exit(1);
}

// 「使用说明」大类的条目不在 Vue 模板里，而在 docs/user-guide.md：面板内联渲染同一份文件，
// 所以这里也按同一份分章口径（src/utils/guideToc.ts）把章 / 节标题收进索引 ——
// 手写一份标题列表必然会随文档漂移。点击后的定位靠 SettingsView 的 jumpToSetting
// （它识别 .guide-h2/.guide-h3，直接滚到正文里的那一节）。
const GUIDE_DOC = join(root, "docs/user-guide.md");
try {
  const guide = splitGuideDocument(readFileSync(GUIDE_DOC, "utf8"));
  for (const c of guide.chapters) {
    for (const title of [c.title, ...c.sections.map((s) => s.title)]) {
      const key = `guide:${title}`;
      if (seen.has(key)) continue;
      seen.add(key);
      items.push({ section: "guide", title });
    }
  }
} catch (err) {
  console.error(`[settings-index] 读取使用说明失败（${GUIDE_DOC}）：${err.message}`);
  process.exit(1);
}

const banner = `/**
 * 设置项索引（**自动生成，请勿手改**）
 *
 * 用途：设置页搜索框。设置页只挂载当前大类，其它大类的设置项不在 DOM 里，
 * 所以搜索需要一份独立索引 —— 它由 SettingsView.vue 的模板提取而来，不会随实现漂移。
 *
 * 生成：npm run gen:settings-index（npm run build 前会自动跑）
 */
export type SettingsIndexEntry = { section: string; title: string }

export const SETTINGS_INDEX: SettingsIndexEntry[] = [
`;

const out =
  banner +
  items.map((it) => `  { section: '${it.section}', title: '${it.title}' },`).join("\n") +
  "\n]\n";

const check = process.argv.includes("--check");
const current = (() => {
  try {
    return readFileSync(OUT, "utf8");
  } catch {
    return "";
  }
})();

if (current === out) {
  console.log(`[settings-index] 已是最新（${items.length} 项 / ${sectionCount} 个分区）`);
  process.exit(0);
}
if (check) {
  console.error("[settings-index] 索引与模板不一致：请运行 npm run gen:settings-index");
  process.exit(1);
}
writeFileSync(OUT, out, "utf8");
console.log(`[settings-index] 已生成 ${items.length} 项（覆盖 ${sectionCount} 个分区）→ ${OUT}`);
