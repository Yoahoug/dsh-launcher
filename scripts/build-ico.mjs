// 从 PNG 生成多尺寸 .ico(PNG 压缩条目,Vista+ 支持),零依赖
// 用法:node scripts/build-ico.mjs <out.ico> <png16> <png32> <png48> <png64> <png128> <png256>
import { readFileSync, writeFileSync } from 'node:fs'

const out = process.argv[2]
const files = process.argv.slice(3)
const entries = files.map((f) => readFileSync(f))

// ICO header: reserved(2) type(2)=1 count(2)
const header = Buffer.alloc(6)
header.writeUInt16LE(0, 0)
header.writeUInt16LE(1, 2)
header.writeUInt16LE(entries.length, 4)

const dir = Buffer.alloc(16 * entries.length)
const blobs = []
let offset = 6 + 16 * entries.length
entries.forEach((png, i) => {
  const size = png.length
  // 尺寸字节:0 表示 256
  const dim = (n) => (n >= 256 ? 0 : n)
  const width = dim(parseInt(/\/(\d+)\.png$/.exec(files[i])?.[1] ?? '256', 10))
  const height = width
  dir.writeUInt8(width, i * 16)
  dir.writeUInt8(height, i * 16 + 1)
  dir.writeUInt8(0, i * 16 + 2) // palette
  dir.writeUInt8(0, i * 16 + 3) // reserved
  dir.writeUInt16LE(1, i * 16 + 4) // planes
  dir.writeUInt16LE(32, i * 16 + 6) // bpp
  dir.writeUInt32LE(size, i * 16 + 8)
  dir.writeUInt32LE(offset, i * 16 + 12)
  blobs.push(png)
  offset += size
})

writeFileSync(out, Buffer.concat([header, dir, ...blobs]))
console.log(`写 ${out}:${entries.length} 个尺寸`)
