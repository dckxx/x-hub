import { test } from 'node:test'
import assert from 'node:assert/strict'
import { looksLikeCode, guessCodeLanguage, unwrapFencedCode } from '../src/utils/pasteCode.ts'

// ---- unwrapFencedCode：整段即围栏才解，夹散文不认 ----

test('unwrapFencedCode 解出整段围栏的语言与内容', () => {
  assert.deepEqual(unwrapFencedCode('```js\nconst a = 1\n```'), {
    code: 'const a = 1',
    language: 'js',
  })
  assert.deepEqual(unwrapFencedCode('\n\n```python\nprint("hi")\n```\n'), {
    code: 'print("hi")',
    language: 'python',
  })
  assert.deepEqual(unwrapFencedCode('~~~ts\nlet x: number = 1\n~~~'), {
    code: 'let x: number = 1',
    language: 'ts',
  })
  // 空内容 / 无语言标注
  assert.deepEqual(unwrapFencedCode('```\n```'), { code: '', language: '' })
  assert.deepEqual(unwrapFencedCode('```bash\nnpm i\n```'), { code: 'npm i', language: 'bash' })
  // 语言行尾带空格/制表符（从文档里复制常见）
  assert.deepEqual(unwrapFencedCode('```js  \nconst a = 1\n```'), {
    code: 'const a = 1',
    language: 'js',
  })
  assert.deepEqual(unwrapFencedCode('~~~ts\t\nlet x = 1\n~~~'), {
    code: 'let x = 1',
    language: 'ts',
  })
})

test('unwrapFencedCode 对散文夹围栏 / 非围栏文本返回 null', () => {
  assert.equal(unwrapFencedCode('这是说明：\n\n```js\nconst a = 1\n```'), null)
  assert.equal(unwrapFencedCode('```js\nconst a = 1\n```\n后面还有文字'), null)
  assert.equal(unwrapFencedCode('普通文本'), null)
})

// ---- looksLikeCode：多行裸代码识别为代码，散文/Markdown/链接不认 ----

test('looksLikeCode 认出常见语言的多行裸代码', () => {
  const js = `function add(a, b) {
    const sum = a + b;
    return sum;
  }`
  const py = `def greet(name):
      msg = f"hello {name}"
      return msg`
  const json = `{
    "name": "x-hub",
    "version": "0.9.0",
    "deps": {
      "vue": "^3.0.0"
    }
  }`
  const bash = `git status
git add .
git commit -m "fix"`
  const sql = `SELECT id, name
FROM resources
WHERE category = 'app'
ORDER BY sort_order;`
  const yaml = `resources:
  limits:
    cpu: 100m
    memory: 128Mi`
  for (const [name, sample] of Object.entries({ js, py, json, bash, sql, yaml })) {
    assert.equal(looksLikeCode(sample), true, `${name} 应识别为代码`)
  }
})

test('looksLikeCode 拒绝散文 / 单行 / Markdown / 链接列表 / 中文为主文本', () => {
  // 中文散文（主要用户群，绝不劫持）
  assert.equal(looksLikeCode('今天天气不错\n我们出去玩吧\n顺便吃个饭'), false)
  // 中文为主的段落里夹一点代码词也不认
  assert.equal(looksLikeCode('我们需要更新这个类\n然后导出报表\n最后删除临时文件'), false)
  // 英文段落
  assert.equal(looksLikeCode('Here is the plan for today.\nWe start with the review.\nThen we ship.'), false)
  // 单行歧义大不认
  assert.equal(looksLikeCode('const a = 1'), false)
  assert.equal(looksLikeCode(''), false)
  // Markdown 列表/标题文档
  assert.equal(looksLikeCode('# Title\n- item one\n- item two\n- item three'), false)
  // 纯链接列表
  assert.equal(looksLikeCode('https://example.com/a\nhttps://example.com/b\nhttps://example.com/c'), false)
  // 夹散文的围栏混合文本不碰（防误吞内容）
  assert.equal(looksLikeCode('说明如下\n```js\nconst a = 1\n```'), false)
})

// ---- guessCodeLanguage：按歧义度逐类识别 ----

test('guessCodeLanguage 识别常见语言', () => {
  assert.equal(guessCodeLanguage('const a = 1;\nconsole.log(a);'), 'javascript')
  assert.equal(guessCodeLanguage('def add(a, b):\n    return a + b'), 'python')
  assert.equal(guessCodeLanguage('{\n  "a": 1,\n  "b": [true, null]\n}'), 'json')
  assert.equal(guessCodeLanguage('npm install\nnpm run build'), 'bash')
  assert.equal(guessCodeLanguage('SELECT * FROM t WHERE id = 1'), 'sql')
  assert.equal(guessCodeLanguage('<!DOCTYPE html>\n<html>\n<body></body>\n</html>'), 'html')
  assert.equal(guessCodeLanguage('interface Foo {\n  bar: string\n}'), 'typescript')
  assert.equal(guessCodeLanguage('public class Main {\n    public static void main(String[] args) {}\n}'), 'java')
  assert.equal(guessCodeLanguage('#include <stdio.h>\nint main() { return 0; }'), 'cpp')
  assert.equal(guessCodeLanguage('随便一段话\n第二行'), '')
})
