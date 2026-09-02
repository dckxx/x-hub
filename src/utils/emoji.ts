/**
 * 速记表情选择器内置语料（零网络，Segoe UI Emoji 全支持）：
 * 供 NoteEditor 斜杠菜单「表情」分组的快捷表情与 EmojiPicker 完整选择器使用。
 *
 * 字段约定：
 *  - e: emoji 字符（含 ZWJ/VS16 组合序列）
 *  - n: 中文名（tooltip 与搜索匹配）
 *  - k: 可选英文关键词（增强搜索，如 laugh / heart / fire）
 */

export interface EmojiItem {
  e: string
  n: string
  k?: string
}

export interface EmojiCategory {
  key: string
  label: string
  items: EmojiItem[]
}

export const EMOJI_CATEGORIES: EmojiCategory[] = [
  {
    key: 'common',
    label: '常用',
    items: [
      { e: '😀', n: '大笑', k: 'grin smile' },
      { e: '😂', n: '笑哭', k: 'joy laugh' },
      { e: '🤣', n: '笑倒', k: 'rofl laugh' },
      { e: '😊', n: '微笑', k: 'blush smile' },
      { e: '😍', n: '花痴', k: 'heart eyes love' },
      { e: '🤔', n: '思考', k: 'thinking hmm' },
      { e: '😭', n: '大哭', k: 'cry sob' },
      { e: '👍', n: '点赞', k: 'thumbs up like' },
      { e: '🙏', n: '感谢', k: 'pray thanks please' },
      { e: '🎉', n: '庆祝', k: 'party tada' },
      { e: '❤️', n: '爱心', k: 'heart love' },
      { e: '🔥', n: '火热', k: 'fire hot' },
      { e: '✨', n: '闪耀', k: 'sparkle star' },
      { e: '💪', n: '加油', k: 'muscle strong' },
      { e: '👏', n: '鼓掌', k: 'clap applause' },
      { e: '😘', n: '亲亲', k: 'kiss blow' },
      { e: '🤗', n: '拥抱', k: 'hug' },
      { e: '😴', n: '睡觉', k: 'sleep tired' },
      { e: '🤡', n: '小丑', k: 'clown' },
      { e: '🥺', n: '委屈', k: 'pleading puppy' },
      { e: '💯', n: '满分', k: '100 perfect' },
      { e: '🆒', n: '很酷', k: 'cool' },
      { e: '🙌', n: '欢呼', k: 'raise hands' },
      { e: '🤝', n: '握手', k: 'handshake deal' },
    ],
  },
  {
    key: 'smiley',
    label: '笑脸',
    items: [
      { e: '😃', n: '开心', k: 'happy' },
      { e: '😄', n: '微笑露齿', k: 'grin' },
      { e: '😁', n: '得意', k: 'beaming' },
      { e: '😆', n: '眯眼笑', k: 'squint laugh' },
      { e: '😅', n: '尴尬笑', k: 'sweat smile' },
      { e: '😉', n: '眨眼', k: 'wink' },
      { e: '🙂', n: '浅笑', k: 'slight smile' },
      { e: '😇', n: '天使', k: 'angel innocent' },
      { e: '🥰', n: '爱意', k: 'smiling hearts' },
      { e: '🤩', n: '星星眼', k: 'star struck' },
      { e: '😋', n: '好吃', k: 'yum tasty' },
      { e: '😜', n: '吐舌', k: 'wink tongue' },
      { e: '🤪', n: '搞怪', k: 'crazy zany' },
      { e: '😐', n: '无语', k: 'neutral meh' },
      { e: '😑', n: '面无表情', k: 'expressionless' },
      { e: '🙄', n: '翻白眼', k: 'roll eyes' },
      { e: '😏', n: '坏笑', k: 'smirk' },
      { e: '😌', n: '惬意', k: 'relieved calm' },
      { e: '😔', n: '郁闷', k: 'pensive sad' },
      { e: '😟', n: '担心', k: 'worried' },
      { e: '😕', n: '困惑', k: 'confused' },
      { e: '😣', n: '纠结', k: 'persevere' },
      { e: '😖', n: '抓狂', k: 'confounded' },
      { e: '😫', n: '疲惫', k: 'tired weary' },
      { e: '😩', n: '崩溃', k: 'weary' },
      { e: '😤', n: '憋气', k: 'triumph steam' },
      { e: '😠', n: '生气', k: 'angry' },
      { e: '😡', n: '愤怒', k: 'rage pout' },
      { e: '🤬', n: '骂人', k: 'cursing swear' },
      { e: '😳', n: '脸红', k: 'flushed' },
      { e: '🤯', n: '爆炸头', k: 'mind blown' },
      { e: '😱', n: '吓到', k: 'scream shocked' },
      { e: '😨', n: '害怕', k: 'fearful' },
      { e: '😰', n: '冒冷汗', k: 'anxious sweat' },
      { e: '🥵', n: '太热', k: 'hot' },
      { e: '🥶', n: '太冷', k: 'cold' },
      { e: '🤢', n: '恶心', k: 'nauseated sick' },
      { e: '🤧', n: '打喷嚏', k: 'sneeze' },
      { e: '😷', n: '戴口罩', k: 'mask' },
      { e: '🤒', n: '发烧', k: 'thermometer ill' },
      { e: '😎', n: '墨镜', k: 'sunglasses cool' },
      { e: '🥳', n: '派对', k: 'party face' },
      { e: '🥹', n: '含泪', k: 'holding back tears' },
    ],
  },
  {
    key: 'gesture',
    label: '手势',
    items: [
      { e: '👋', n: '挥手', k: 'wave hello bye' },
      { e: '🤚', n: '举手', k: 'raised hand' },
      { e: '✋', n: '手掌', k: 'hand palm' },
      { e: '🖐️', n: '张开手', k: 'spread hand' },
      { e: '✌️', n: '胜利', k: 'victory peace' },
      { e: '🤞', n: '好运', k: 'crossed fingers luck' },
      { e: '🤟', n: '我爱你', k: 'love you' },
      { e: '🤘', n: '摇滚', k: 'rock horn metal' },
      { e: '👌', n: '好的', k: 'ok perfect' },
      { e: '🤌', n: '捏手指', k: 'pinched' },
      { e: '👎', n: '踩', k: 'thumbs down' },
      { e: '👊', n: '拳头', k: 'fist punch' },
      { e: '✊', n: '握拳', k: 'raised fist' },
      { e: '🤛', n: '左拳', k: 'left fist' },
      { e: '🤜', n: '右拳', k: 'right fist' },
      { e: '🤙', n: '打电话', k: 'call me shaka' },
      { e: '🫶', n: '爱心手势', k: 'heart hands' },
      { e: '💅', n: '美甲', k: 'nail care' },
      { e: '👏', n: '鼓掌', k: 'clap' },
      { e: '🙌', n: '举起双手', k: 'raise hands' },
      { e: '🙏', n: '拜托', k: 'pray thanks' },
      { e: '🤝', n: '握手', k: 'handshake' },
      { e: '💪', n: '肌肉', k: 'muscle biceps' },
      { e: '🦾', n: '机械臂', k: 'mechanical arm' },
    ],
  },
  {
    key: 'animal',
    label: '动物自然',
    items: [
      { e: '🐶', n: '狗', k: 'dog puppy' },
      { e: '🐱', n: '猫', k: 'cat kitty' },
      { e: '🐭', n: '老鼠', k: 'mouse' },
      { e: '🐹', n: '仓鼠', k: 'hamster' },
      { e: '🐰', n: '兔子', k: 'rabbit bunny' },
      { e: '🦊', n: '狐狸', k: 'fox' },
      { e: '🐻', n: '熊', k: 'bear' },
      { e: '🐼', n: '熊猫', k: 'panda' },
      { e: '🐨', n: '考拉', k: 'koala' },
      { e: '🐯', n: '老虎', k: 'tiger' },
      { e: '🦁', n: '狮子', k: 'lion' },
      { e: '🐮', n: '牛', k: 'cow' },
      { e: '🐷', n: '猪', k: 'pig' },
      { e: '🐸', n: '青蛙', k: 'frog' },
      { e: '🐵', n: '猴子', k: 'monkey' },
      { e: '🐔', n: '鸡', k: 'chicken' },
      { e: '🐧', n: '企鹅', k: 'penguin' },
      { e: '🐦', n: '鸟', k: 'bird' },
      { e: '🦉', n: '猫头鹰', k: 'owl' },
      { e: '🦄', n: '独角兽', k: 'unicorn' },
      { e: '🐢', n: '乌龟', k: 'turtle' },
      { e: '🐳', n: '鲸鱼', k: 'whale' },
      { e: '🐬', n: '海豚', k: 'dolphin' },
      { e: '🦋', n: '蝴蝶', k: 'butterfly' },
      { e: '🐝', n: '蜜蜂', k: 'bee honey' },
      { e: '🌵', n: '仙人掌', k: 'cactus' },
      { e: '🌸', n: '樱花', k: 'cherry blossom' },
      { e: '🌹', n: '玫瑰', k: 'rose' },
      { e: '🌻', n: '向日葵', k: 'sunflower' },
      { e: '🌈', n: '彩虹', k: 'rainbow' },
      { e: '⭐', n: '星星', k: 'star' },
      { e: '🌟', n: '闪星', k: 'glowing star' },
      { e: '🌙', n: '月亮', k: 'moon night' },
      { e: '☀️', n: '太阳', k: 'sun' },
      { e: '⛅', n: '多云', k: 'cloudy' },
      { e: '🌊', n: '海浪', k: 'wave ocean' },
      { e: '⛄', n: '雪人', k: 'snowman' },
      { e: '☔', n: '雨伞', k: 'umbrella rain' },
    ],
  },
  {
    key: 'food',
    label: '食物',
    items: [
      { e: '🍎', n: '苹果', k: 'apple' },
      { e: '🍌', n: '香蕉', k: 'banana' },
      { e: '🍉', n: '西瓜', k: 'watermelon' },
      { e: '🍇', n: '葡萄', k: 'grapes' },
      { e: '🍓', n: '草莓', k: 'strawberry' },
      { e: '🍒', n: '樱桃', k: 'cherry' },
      { e: '🍑', n: '桃子', k: 'peach' },
      { e: '🥭', n: '芒果', k: 'mango' },
      { e: '🍍', n: '菠萝', k: 'pineapple' },
      { e: '🥝', n: '猕猴桃', k: 'kiwi' },
      { e: '🍅', n: '番茄', k: 'tomato' },
      { e: '🥑', n: '牛油果', k: 'avocado' },
      { e: '🍞', n: '面包', k: 'bread' },
      { e: '🧀', n: '奶酪', k: 'cheese' },
      { e: '🍔', n: '汉堡', k: 'burger' },
      { e: '🍟', n: '薯条', k: 'fries' },
      { e: '🍕', n: '披萨', k: 'pizza' },
      { e: '🌭', n: '热狗', k: 'hotdog' },
      { e: '🍜', n: '拉面', k: 'noodles ramen' },
      { e: '🍣', n: '寿司', k: 'sushi' },
      { e: '🍱', n: '便当', k: 'bento box' },
      { e: '🍚', n: '米饭', k: 'rice' },
      { e: '🍧', n: '刨冰', k: 'shaved ice' },
      { e: '🍦', n: '冰淇淋', k: 'ice cream' },
      { e: '🎂', n: '蛋糕', k: 'birthday cake' },
      { e: '🍰', n: '蛋糕片', k: 'shortcake' },
      { e: '🍫', n: '巧克力', k: 'chocolate' },
      { e: '🍬', n: '糖果', k: 'candy' },
      { e: '🍭', n: '棒棒糖', k: 'lollipop' },
      { e: '☕', n: '咖啡', k: 'coffee tea' },
      { e: '🍵', n: '绿茶', k: 'tea matcha' },
      { e: '🧋', n: '奶茶', k: 'bubble tea milk' },
      { e: '🧃', n: '果汁', k: 'juice box' },
      { e: '🍺', n: '啤酒', k: 'beer' },
    ],
  },
  {
    key: 'activity',
    label: '活动物品',
    items: [
      { e: '⚽', n: '足球', k: 'soccer football' },
      { e: '🏀', n: '篮球', k: 'basketball' },
      { e: '🏈', n: '橄榄球', k: 'football' },
      { e: '⚾', n: '棒球', k: 'baseball' },
      { e: '🎾', n: '网球', k: 'tennis' },
      { e: '🏐', n: '排球', k: 'volleyball' },
      { e: '🎱', n: '台球', k: 'billiards 8ball' },
      { e: '🏓', n: '乒乓球', k: 'ping pong' },
      { e: '🎮', n: '游戏', k: 'game controller' },
      { e: '🎲', n: '骰子', k: 'dice' },
      { e: '🎧', n: '耳机', k: 'headphone music' },
      { e: '🎤', n: '麦克风', k: 'microphone sing' },
      { e: '🎸', n: '吉他', k: 'guitar' },
      { e: '🎹', n: '钢琴', k: 'piano keyboard' },
      { e: '📱', n: '手机', k: 'phone mobile' },
      { e: '💻', n: '电脑', k: 'laptop computer' },
      { e: '⌚', n: '手表', k: 'watch' },
      { e: '📷', n: '相机', k: 'camera photo' },
      { e: '🎁', n: '礼物', k: 'gift present' },
      { e: '🎈', n: '气球', k: 'balloon' },
      { e: '💡', n: '灯泡', k: 'lightbulb idea' },
      { e: '🔑', n: '钥匙', k: 'key' },
      { e: '💰', n: '钱袋', k: 'money bag' },
      { e: '📚', n: '书本', k: 'books study' },
      { e: '✏️', n: '铅笔', k: 'pencil write' },
      { e: '📌', n: '图钉', k: 'pin' },
      { e: '🔔', n: '铃铛', k: 'bell' },
      { e: '🎯', n: '靶心', k: 'target goal' },
    ],
  },
  {
    key: 'place',
    label: '旅行地点',
    items: [
      { e: '🚗', n: '汽车', k: 'car' },
      { e: '🚕', n: '出租车', k: 'taxi' },
      { e: '🚌', n: '公交车', k: 'bus' },
      { e: '🚓', n: '警车', k: 'police car' },
      { e: '🚑', n: '救护车', k: 'ambulance' },
      { e: '🚒', n: '消防车', k: 'fire truck' },
      { e: '🚲', n: '自行车', k: 'bicycle bike' },
      { e: '✈️', n: '飞机', k: 'airplane flight' },
      { e: '🚀', n: '火箭', k: 'rocket' },
      { e: '🛸', n: '飞碟', k: 'ufo flying saucer' },
      { e: '🚁', n: '直升机', k: 'helicopter' },
      { e: '⛵', n: '帆船', k: 'sailboat' },
      { e: '🚢', n: '轮船', k: 'ship boat' },
      { e: '🏠', n: '家', k: 'house home' },
      { e: '🏢', n: '办公楼', k: 'office building' },
      { e: '🏥', n: '医院', k: 'hospital' },
      { e: '🏫', n: '学校', k: 'school' },
      { e: '🏰', n: '城堡', k: 'castle' },
      { e: '🗼', n: '铁塔', k: 'eiffel tower' },
      { e: '🗽', n: '自由女神', k: 'statue liberty' },
      { e: '🌋', n: '火山', k: 'volcano' },
      { e: '🏔️', n: '雪山', k: 'snow mountain' },
      { e: '🏖️', n: '海滩', k: 'beach' },
      { e: '🎡', n: '摩天轮', k: 'ferris wheel' },
      { e: '🎢', n: '过山车', k: 'roller coaster' },
    ],
  },
  {
    key: 'symbol',
    label: '符号',
    items: [
      { e: '❤️', n: '红心', k: 'heart' },
      { e: '🧡', n: '橙心', k: 'orange heart' },
      { e: '💛', n: '黄心', k: 'yellow heart' },
      { e: '💚', n: '绿心', k: 'green heart' },
      { e: '💙', n: '蓝心', k: 'blue heart' },
      { e: '💜', n: '紫心', k: 'purple heart' },
      { e: '🖤', n: '黑心', k: 'black heart' },
      { e: '🤍', n: '白心', k: 'white heart' },
      { e: '💔', n: '心碎', k: 'broken heart' },
      { e: '💕', n: '两颗心', k: 'two hearts' },
      { e: '💞', n: '旋转心', k: 'revolving hearts' },
      { e: '💖', n: '闪亮心', k: 'sparkling heart' },
      { e: '✅', n: '对勾', k: 'check done' },
      { e: '❌', n: '叉号', k: 'cross no' },
      { e: '⚠️', n: '警告', k: 'warning' },
      { e: '❓', n: '问号', k: 'question' },
      { e: '❗', n: '感叹号', k: 'exclamation' },
      { e: '💯', n: '百分', k: '100 percent' },
      { e: '⚡', n: '闪电', k: 'lightning zap' },
      { e: '💤', n: '睡觉', k: 'zzz sleeping' },
      { e: '💧', n: '水滴', k: 'droplet water' },
      { e: '🕐', n: '一点', k: 'one clock time' },
      { e: '⏰', n: '闹钟', k: 'alarm clock' },
      { e: '📅', n: '日历', k: 'calendar date' },
    ],
  },
]

/** 斜杠菜单「表情」分组的快捷表情数量（第 9 个固定为「更多表情…」入口） */
export const QUICK_EMOJI_COUNT = 8

/** 缺字检测结果缓存：emoji -> 当前系统能否正常渲染 */
const RENDER_CACHE = new Map<string, boolean>()

/**
 * 检测当前系统字体能否渲染该表情：把表情画到离屏画布上，与「豆腐块」（无字形
 * 码点的基准图）和「纯空白」比对，像素一致即视为缺字（用户看到的框框）。
 * 检测不了或异常时保守放行，宁可显示也不误删。
 */
function emojiRenderable(e: string): boolean {
  const cached = RENDER_CACHE.get(e)
  if (cached !== undefined) return cached
  let ok = true
  try {
    const canvas = document.createElement('canvas')
    canvas.width = canvas.height = 24
    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) {
      RENDER_CACHE.set(e, true)
      return true
    }
    ctx.textBaseline = 'middle'
    ctx.font = '20px "Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", sans-serif'
    const draw = (s: string) => {
      ctx.clearRect(0, 0, 24, 24)
      ctx.fillStyle = '#000'
      ctx.fillText(s, 2, 13)
      return ctx.getImageData(0, 0, 24, 24).data.join(',')
    }
    const tofu = draw('\u{10FFFD}') // 无字形码点 → 系统缺字豆腐块基准
    const blank = draw(' ') // 空白基准（部分平台缺字画成透明）
    const pixels = draw(e)
    ok = pixels !== tofu && pixels !== blank
  } catch {
    ok = true
  }
  RENDER_CACHE.set(e, ok)
  return ok
}

/**
 * 快捷表情：「最近使用」优先（用户自己的高频），不足从「常用」默认语料补齐，
 * 保证恰好 QUICK_EMOJI_COUNT 个。系统字体缺字（显示为框框）的表情自动跳过。
 * 编辑器挂载/斜杠菜单构建时重新求值。
 */
export function getQuickEmojis(): EmojiItem[] {
  const defaults = EMOJI_CATEGORIES.find((c) => c.key === 'common')?.items ?? []
  const out: EmojiItem[] = []
  const seen = new Set<string>()
  for (const it of [...getRecentEmojis(), ...defaults]) {
    if (seen.has(it.e)) continue
    seen.add(it.e)
    if (!emojiRenderable(it.e)) continue
    out.push(it)
    if (out.length >= QUICK_EMOJI_COUNT) break
  }
  return out
}

/** 全量索引：e -> item（供最近使用解析与去重） */
const EMOJI_INDEX = new Map<string, EmojiItem>(
  EMOJI_CATEGORIES.flatMap((c) => c.items).map((i) => [i.e, i]),
)

const RECENT_KEY = 'xhub_recent_emojis'
const RECENT_MAX = 24

/** 读取最近使用表情（localStorage 持久化，按使用先后倒序） */
export function getRecentEmojis(): EmojiItem[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY)
    if (!raw) return []
    const arr = JSON.parse(raw) as string[]
    return arr
      .map((e) => EMOJI_INDEX.get(e))
      .filter((x): x is EmojiItem => !!x)
  } catch {
    return []
  }
}

/** 记录一次表情使用：置顶去重，超出上限裁剪尾部 */
export function pushRecentEmoji(e: string) {
  try {
    const arr = [e, ...getRecentEmojis().map((i) => i.e).filter((x) => x !== e)].slice(0, RECENT_MAX)
    localStorage.setItem(RECENT_KEY, JSON.stringify(arr))
  } catch {
    /* 存储不可用时静默 */
  }
}

/** 按关键词（中文名 + 英文）过滤表情 */
export function filterEmojis(list: EmojiItem[], q: string): EmojiItem[] {
  const s = q.trim().toLowerCase()
  if (!s) return list
  return list.filter((i) => i.n.toLowerCase().includes(s) || (i.k?.toLowerCase().includes(s) ?? false))
}
