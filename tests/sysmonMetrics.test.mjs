/**
 * 系统资源卡片指标口径（src/utils/sysmonMetrics.ts）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码（零运行时依赖）：
 *   node --test tests/sysmonMetrics.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import {
  DEFAULT_SYSMON_METRICS,
  fmtBytes,
  fmtSpeed,
  resolveSysmonMetrics,
  SYSMON_METRIC_DEFS,
  sysmonMetricDisplay,
} from '../src/utils/sysmonMetrics.ts'

const SAMPLE = {
  cpuUsage: 37.4,
  memPercent: 62.9,
  netUpload: 2048,
  netDownload: 1572864,
  netTotalSpeed: 1574912,
  netTotalTraffic: 5368709120,
  cpuThreads: 16,
}

test('resolve：空配置回退内置默认（顺序即默认顺序）', () => {
  assert.deepEqual(resolveSysmonMetrics([]).map((m) => m.id), DEFAULT_SYSMON_METRICS)
  assert.deepEqual(resolveSysmonMetrics(undefined).map((m) => m.id), DEFAULT_SYSMON_METRICS)
  assert.deepEqual(resolveSysmonMetrics(null).map((m) => m.id), DEFAULT_SYSMON_METRICS)
})

test('resolve：过滤非法 id 并按配置顺序排列', () => {
  const out = resolveSysmonMetrics(['traffic', 'nope', 'cpu'])
  assert.deepEqual(
    out.map((m) => m.id),
    ['traffic', 'cpu'],
  )
})

test('resolve：去重（重复 id 只保留首个）', () => {
  const out = resolveSysmonMetrics(['cpu', 'cpu', 'mem', 'cpu'])
  assert.deepEqual(
    out.map((m) => m.id),
    ['cpu', 'mem'],
  )
})

test('resolve：旧版独立上传/下载 id 迁移为合并的「上传/下载」一项', () => {
  assert.deepEqual(resolveSysmonMetrics(['net_upload', 'cpu']).map((m) => m.id), ['net_rate', 'cpu'])
  assert.deepEqual(resolveSysmonMetrics(['net_download']).map((m) => m.id), ['net_rate'])
  // 上传下载都配置过 → 合并后只落一项（映射后去重）
  assert.deepEqual(resolveSysmonMetrics(['net_upload', 'net_download', 'mem']).map((m) => m.id), [
    'net_rate',
    'mem',
  ])
})

test('resolve：全部非法同样回退默认，绝不渲染空卡片', () => {
  assert.deepEqual(resolveSysmonMetrics(['x', 'y']).map((m) => m.id), DEFAULT_SYSMON_METRICS)
})

test('目录：默认列表里的 id 都真实存在（防手滑改 id 后默认失效）', () => {
  const ids = new Set(SYSMON_METRIC_DEFS.map((d) => d.id))
  for (const id of DEFAULT_SYSMON_METRICS) assert.ok(ids.has(id), `默认指标 ${id} 不在目录中`)
})

test('fmtBytes：单位自适应与边界', () => {
  assert.equal(fmtBytes(0), '0 B')
  assert.equal(fmtBytes(-5), '0 B')
  assert.equal(fmtBytes(1023), '1023 B')
  assert.equal(fmtBytes(1024), '1.0 KB')
  assert.equal(fmtBytes(1572864), '1.5 MB')
  assert.equal(fmtBytes(5368709120), '5.0 GB')
})

test('fmtSpeed：速率 = 字节数 + /s 后缀', () => {
  assert.equal(fmtSpeed(0), '0 B/s')
  assert.equal(fmtSpeed(1572864), '1.5 MB/s')
})

test('展示值：bar 型带钳制百分比，value 型只有文本与单位', () => {
  const cpu = sysmonMetricDisplay('cpu', SAMPLE)
  assert.equal(cpu.text, '37')
  assert.equal(cpu.unit, '%')
  assert.equal(cpu.pct, 37.4)

  const down = sysmonMetricDisplay('net_download', SAMPLE)
  assert.equal(down.text, '1.5')
  assert.equal(down.unit, 'MB/s')
  assert.equal(down.pct, undefined)

  assert.equal(sysmonMetricDisplay('cpu_threads', SAMPLE).text, '16')
  assert.equal(sysmonMetricDisplay('cpu_threads', SAMPLE).unit, '线程')
  assert.equal(sysmonMetricDisplay('traffic', SAMPLE).text, '5.0')
  assert.equal(sysmonMetricDisplay('traffic', SAMPLE).unit, 'GB')
})

test('展示值：采样未就绪与超界输入都有兜底', () => {
  assert.equal(sysmonMetricDisplay('cpu', null).text, '—')
  assert.equal(sysmonMetricDisplay('unknown', SAMPLE).text, '—')
  const hot = sysmonMetricDisplay('cpu', { ...SAMPLE, cpuUsage: 130 })
  assert.equal(hot.text, '100')
  assert.equal(hot.pct, 100)
  const nan = sysmonMetricDisplay('net_upload', { ...SAMPLE, netUpload: NaN })
  assert.equal(nan.text, '0')
  assert.equal(nan.unit, 'B/s')
})
