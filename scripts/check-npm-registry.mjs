/**
 * 锁文件里的下载地址只能是公网 npm。
 * 内网源（例如 mirrors.casiosz.cn）在 GitHub CI 上解析不了，npm ci 会失败。
 *
 *   node scripts/check-npm-registry.mjs          发现内网源就失败
 *   node scripts/check-npm-registry.mjs --fix    先改成 registry.npmjs.org 再检查
 */
import { readFileSync, writeFileSync, existsSync } from 'node:fs'

const ALLOWED = new Set(['registry.npmjs.org', 'registry.npmmirror.com'])
const LOCK = 'package-lock.json'
const fix = process.argv.includes('--fix')

function hostOf(url) {
  try {
    return new URL(url).host
  } catch {
    return ''
  }
}

function toPublic(url) {
  const parsed = new URL(url)
  let path = parsed.pathname
  const marker = '/npm/'
  const at = path.indexOf(marker)
  if (at >= 0) path = path.slice(at + marker.length - 1)
  return `https://registry.npmjs.org${path}`
}

function resolvedUrls(text) {
  const found = []
  const re = /"resolved"\s*:\s*"(https?:\/\/[^"]+)"/g
  let match
  while ((match = re.exec(text))) found.push(match[1])
  return found
}

if (!existsSync(LOCK)) {
  console.error(`缺少 ${LOCK}`)
  process.exit(1)
}

let text = readFileSync(LOCK, 'utf8')
if (fix) {
  const next = text.replace(/"resolved"\s*:\s*"(https?:\/\/[^"]+)"/g, (full, url) => {
    if (ALLOWED.has(hostOf(url))) return full
    return full.replace(url, toPublic(url))
  })
  if (next !== text) {
    writeFileSync(LOCK, next)
    text = next
    console.log(`已把 ${LOCK} 里的内网源改成 https://registry.npmjs.org/`)
  }
}

const blocked = [...new Set(resolvedUrls(text).filter((url) => !ALLOWED.has(hostOf(url))))]
if (blocked.length === 0) {
  console.log('npm 源检查通过')
  process.exit(0)
}

console.error('package-lock.json 含有内网或未允许的 npm 源，不能提交：')
for (const url of blocked) console.error(`  ${url}`)
console.error('允许的主机：registry.npmjs.org、registry.npmmirror.com')
console.error('可先运行 node scripts/check-npm-registry.mjs --fix')
process.exit(1)
