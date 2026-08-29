/**
 * 标签归一化工具（BUG1：looking at viewer / looking_at_viewer 同义）。
 * - 存储/词典/搜索用规范名（下划线）；展示时转空格更美观。
 * - 搜索输入空格 → 转下划线查询。
 */

/** 展示归一化：下划线 → 空格（仅当不含中文时，避免误伤中文标签）。 */
export function displayTagName(name: string): string {
  // 含非 ASCII（中文等）直接返回
  if (/[^\x00-\x7F]/.test(name)) return name
  return name.replace(/_/g, ' ')
}

/** 查询归一化：空格 → 下划线（规范名）。 */
export function searchTagKey(name: string): string {
  return name.trim().replace(/ /g, '_')
}

/** 展示标签（含中文名，支持优先中文设置）。 */
export function displayTagLabel(name: string, nameCn: string | null, cnFirst: boolean): string {
  if (!nameCn) return displayTagName(name)
  const en = displayTagName(name)
  return cnFirst ? `${nameCn}(${en})` : `${en}(${nameCn})`
}
