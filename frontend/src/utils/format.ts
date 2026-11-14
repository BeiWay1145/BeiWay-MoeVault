/**
 * 通用格式化工具（从 ImageDetailView.vue 抽出）。
 */

/** 文件大小格式化（B / KB / MB / GB）。 */
export function fmtBytes(b: number): string {
  if (b >= 1 << 30) return `${(b / (1 << 30)).toFixed(2)} GB`
  if (b >= 1 << 20) return `${(b / (1 << 20)).toFixed(1)} MB`
  if (b >= 1 << 10) return `${(b / (1 << 10)).toFixed(0)} KB`
  return `${b} B`
}
