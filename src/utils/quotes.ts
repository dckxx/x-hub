// 本地内置名言金句语料（离线兜底 / local 模式轮换用）。
// 纯通用名言金句，不出网、不依赖任何服务。

export interface LocalQuote {
  content: string
  from: string
}

export const LOCAL_QUOTES: LocalQuote[] = [
  { content: '日拱一卒，功不唐捐。', from: '格言' },
  { content: '路漫漫其修远兮，吾将上下而求索。', from: '屈原·离骚' },
  { content: '宝剑锋从磨砺出，梅花香自苦寒来。', from: '警世贤文' },
  { content: '千里之行，始于足下。', from: '老子' },
  { content: '不积跬步，无以至千里。', from: '荀子·劝学' },
  { content: '锲而不舍，金石可镂。', from: '荀子·劝学' },
  { content: '业精于勤，荒于嬉；行成于思，毁于随。', from: '韩愈·进学解' },
  { content: '博观而约取，厚积而薄发。', from: '苏轼' },
  { content: '纸上得来终觉浅，绝知此事要躬行。', from: '陆游' },
  { content: '长风破浪会有时，直挂云帆济沧海。', from: '李白·行路难' },
  { content: '天生我材必有用，千金散尽还复来。', from: '李白·将进酒' },
  { content: '会当凌绝顶，一览众山小。', from: '杜甫·望岳' },
  { content: '山重水复疑无路，柳暗花明又一村。', from: '陆游·游山西村' },
  { content: '沉舟侧畔千帆过，病树前头万木春。', from: '刘禹锡' },
  { content: '少壮不努力，老大徒伤悲。', from: '长歌行' },
  { content: '学而不思则罔，思而不学则殆。', from: '论语·为政' },
  { content: '温故而知新，可以为师矣。', from: '论语' },
  { content: '知者不惑，仁者不忧，勇者不惧。', from: '论语' },
  { content: '己所不欲，勿施于人。', from: '论语' },
  { content: '天行健，君子以自强不息。', from: '周易' },
  { content: '地势坤，君子以厚德载物。', from: '周易' },
  { content: '志不强者智不达。', from: '墨子' },
  { content: '穷则独善其身，达则兼济天下。', from: '孟子' },
  { content: '老骥伏枥，志在千里。', from: '曹操·龟虽寿' },
]

let lastIndex = -1

/** 随机取一条本地语料，避免与上一条重复 */
export function randomLocalQuote(): LocalQuote {
  if (LOCAL_QUOTES.length === 0) return { content: '', from: '' }
  if (LOCAL_QUOTES.length === 1) return LOCAL_QUOTES[0]
  let idx = Math.floor(Math.random() * LOCAL_QUOTES.length)
  if (idx === lastIndex) idx = (idx + 1) % LOCAL_QUOTES.length
  lastIndex = idx
  return LOCAL_QUOTES[idx]
}
