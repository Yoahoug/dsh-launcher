// dsh-launcher · 零依赖 ZIP 解压(node:zlib inflateRaw + 中央目录解析)
// 支持 store(0)与 deflate(8),拒绝路径穿越;供内置更新器解压发布包。
import { createWriteStream, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, normalize, resolve, sep } from 'node:path'
import { inflateRawSync } from 'node:zlib'

const EOCD_SIG = 0x06054b50
const CENTRAL_SIG = 0x02014b50
const LOCAL_SIG = 0x04034b50

/** 从尾部扫描 EOCD(最多 64KB+22 字节)。 */
function findEocd(buf) {
  const min = Math.max(0, buf.length - 65557)
  for (let i = buf.length - 22; i >= min; i--) {
    if (buf.readUInt32LE(i) === EOCD_SIG) return i
  }
  throw new Error('不是有效的 zip(找不到中央目录)')
}

/** 安全目标路径:拒绝绝对路径与 .. 穿越。 */
function safeTarget(root, entryPath) {
  const norm = normalize(entryPath).replace(/^[\\/]+/, '')
  if (!norm || norm === '.' || norm.includes('..')) {
    throw new Error(`zip 条目路径不安全:${entryPath}`)
  }
  return resolve(root, norm)
}

/**
 * 解压 zip 到目标目录(覆盖写)。
 * @param {Buffer|string} zipBufOrPath
 * @param {string} outDir
 * @returns {Promise<string[]>} 解出的文件相对路径
 */
export function unzip(zipBufOrPath, outDir) {
  const buf = typeof zipBufOrPath === 'string' ? readFileSync(zipBufOrPath) : zipBufOrPath
  const eocd = findEocd(buf)
  const entryCount = buf.readUInt16LE(eocd + 10)
  const cdOffset = buf.readUInt32LE(eocd + 16)
  const written = []

  let p = cdOffset
  for (let i = 0; i < entryCount; i++) {
    if (buf.readUInt32LE(p) !== CENTRAL_SIG) throw new Error(`中央目录损坏(条目 ${i})`)
    const method = buf.readUInt16LE(p + 10)
    const compSize = buf.readUInt32LE(p + 20)
    const nameLen = buf.readUInt16LE(p + 28)
    const extraLen = buf.readUInt16LE(p + 30)
    const commentLen = buf.readUInt16LE(p + 32)
    const localOffset = buf.readUInt32LE(p + 42)
    const name = buf.toString('utf8', p + 46, p + 46 + nameLen)

    if (name.endsWith('/')) { // 目录条目
      mkdirSync(safeTarget(outDir, name), { recursive: true })
    } else {
      // 解析本地头,取数据起始
      if (buf.readUInt32LE(localOffset) !== LOCAL_SIG) throw new Error(`本地头损坏:${name}`)
      const lNameLen = buf.readUInt16LE(localOffset + 26)
      const lExtraLen = buf.readUInt16LE(localOffset + 28)
      const dataStart = localOffset + 30 + lNameLen + lExtraLen
      const data = buf.subarray(dataStart, dataStart + compSize)
      const out = safeTarget(outDir, name)
      mkdirSync(dirname(out), { recursive: true })
      if (method === 0) {
        writeFileSync(out, data)
      } else if (method === 8) {
        writeFileSync(out, inflateRawSync(data))
      } else {
        throw new Error(`不支持的压缩方式 ${method}:${name}`)
      }
      written.push(name)
    }
    p += 46 + nameLen + extraLen + commentLen
  }
  return written
}

/** 便于流式下载后直接解压。 */
export function unzipFromFile(zipPath, outDir) {
  return unzip(zipPath, outDir)
}
