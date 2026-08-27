#!/usr/bin/env node
/**
 * Ed25519 分离签名工具（发布侧）。
 * 用法: node pub-sign.mjs <私钥PEM路径> <输入文件> <输出签名文件(.sig)>
 * 输出: 对输入文件**原始字节**的 64 字节 Ed25519 签名，base64 文本写入输出文件。
 * 客户端（signing.rs）用内嵌公钥验证同一原始字节，两端使用同一密钥对。
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { createPrivateKey, sign } from 'node:crypto'

const [privKeyPath, inputPath, outPath] = process.argv.slice(2)
if (!privKeyPath || !inputPath || !outPath) {
  console.error('用法: node pub-sign.mjs <私钥PEM> <输入文件> <输出.sig>')
  process.exit(1)
}

const key = createPrivateKey(readFileSync(privKeyPath))
const data = readFileSync(inputPath)
const sig = sign(null, data, key)
writeFileSync(outPath, sig.toString('base64') + '\n')
console.log(`已签名: ${inputPath} -> ${outPath} (${sig.length} bytes, base64)`)