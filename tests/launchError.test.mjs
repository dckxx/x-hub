/**
 * 速达启动错误判定（src/utils/launchError.ts）单测。
 * 直接 node:test + 原生类型剥离跑 TS 源码：
 *   node --test tests/launchError.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { isTargetMissingError } from '../src/utils/launchError.ts'

test('识别后端 TARGET_NOT_FOUND（应用已卸载/移动）', () => {
  assert.ok(isTargetMissingError('TARGET_NOT_FOUND: 找不到对应程序「C:\\App\\a.exe」'))
  assert.ok(
    isTargetMissingError(
      new Error('TARGET_NOT_FOUND: 找不到对应应用「shell:AppsFolder\\X!Y」'),
    ),
  )
})

test('普通启动错误不误判为「应用不存在」', () => {
  assert.ok(!isTargetMissingError('启动程序失败「C:\\App\\a.exe」: 拒绝访问'))
  assert.ok(!isTargetMissingError('打开链接失败: 没有与之关联的应用'))
  assert.ok(!isTargetMissingError(''))
  assert.ok(!isTargetMissingError(null))
  assert.ok(!isTargetMissingError(undefined))
})
