import { computed, ref, type Ref } from 'vue'

/**
 * 统一的选择模型（图库 / 主目录 / 未来的搜索页共用）。
 *
 * 设计目标（资源管理器风格）：
 * - **唯一入口**：所有选择来源（点击、快捷键、框选、右键、外部设置）都收敛到 applySelection /
 *   selectRange / selectByRect / setSelection，避免各页面各写一套导致行为漂移。
 * - **锚点语义**：Shift 范围选择需要"上一次点击的位置"，由 anchor 记录。
 * - **有序依赖**：范围选择与全选都依赖"当前显示顺序"，由调用方通过 getOrderedIds 注入，
 *   这样瀑布流/网格/列表三种布局无需各自实现。
 *
 * 使用方（图库）：
 *   const sel = useSelection(() => library.images.map((i) => i.id))
 */
export function useSelection(getOrderedIds: () => number[]) {
  /** 已选中的图片 id 集合。 */
  const selected = ref<Set<number>>(new Set())
  /** Shift 范围选择的锚点（上次单击的图片 id）。 */
  const anchor = ref<number | null>(null)

  const count = computed(() => selected.value.size)
  const isEmpty = computed(() => selected.value.size === 0)

  /** 内部：整体替换选中集（触发响应式）。 */
  function replace(next: Set<number>) {
    selected.value = next
  }

  /**
   * 唯一的选择入口。
   *
   * @param id    被点击的图片 id
   * @param mods  ctrl = 叠加切换；shift = 从锚点范围选择；两者皆无 = 替换式单选
   *
   * 行为矩阵（与资源管理器一致）：
   * - 无修饰键           → 只选中该图（清空其它），并更新锚点
   * - Ctrl              → 切换该图选中态（叠加），并更新锚点
   * - Shift             → 选中 anchor→id 的连续区间（替换式）；无锚点时退化为单选
   * - Ctrl+Shift        → 选中 anchor→id 的连续区间并**追加**到现有选择
   */
  function applySelection(id: number, mods: { ctrl?: boolean; shift?: boolean } = {}) {
    const { ctrl = false, shift = false } = mods

    if (shift) {
      const from = anchor.value
      if (from === null || from === id) {
        // 无锚点（或与锚点相同）：退化为单选
        replace(new Set([id]))
        anchor.value = id
        return
      }
      const range = rangeIds(from, id)
      if (ctrl) {
        // Ctrl+Shift：在现有选择上追加区间
        const next = new Set(selected.value)
        range.forEach((x) => next.add(x))
        replace(next)
      } else {
        replace(new Set(range))
      }
      // Shift 不改变锚点（方便连续调整终点）
      return
    }

    if (ctrl) {
      const next = new Set(selected.value)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      replace(next)
      anchor.value = id
      return
    }

    replace(new Set([id]))
    anchor.value = id
  }

  /** 计算 from→to 在当前显示顺序中的连续区间（含两端；方向自适应）。 */
  function rangeIds(from: number, to: number): number[] {
    const order = getOrderedIds()
    const a = order.indexOf(from)
    const b = order.indexOf(to)
    if (a < 0 || b < 0) return [to]
    const lo = Math.min(a, b)
    const hi = Math.max(a, b)
    return order.slice(lo, hi + 1)
  }

  /** 范围选择（显式指定两端，供框选/程序调用）。 */
  function selectRange(from: number, to: number, additive = false) {
    const range = rangeIds(from, to)
    if (additive) {
      const next = new Set(selected.value)
      range.forEach((x) => next.add(x))
      replace(next)
    } else {
      replace(new Set(range))
    }
    anchor.value = from
  }

  /** 全选当前显示列表。 */
  function selectAll() {
    replace(new Set(getOrderedIds()))
  }

  /** 清空选择与锚点。 */
  function clear() {
    replace(new Set())
    anchor.value = null
  }

  /** 直接设置选择集（外部驱动，如详情页删除后收缩、右键临时选中）。 */
  function setSelection(ids: Iterable<number>) {
    replace(new Set(ids))
  }

  /** 追加选择（不改变锚点）。 */
  function addToSelection(ids: Iterable<number>) {
    const next = new Set(selected.value)
    for (const id of ids) next.add(id)
    replace(next)
  }

  /** 从选择集移除。 */
  function removeFromSelection(ids: Iterable<number>) {
    const next = new Set(selected.value)
    for (const id of ids) next.delete(id)
    replace(next)
  }

  /** 切换单张（右键/复选框等既有语义，保留兼容）。 */
  function toggle(id: number) {
    applySelection(id, { ctrl: true })
  }

  /** 当前显示列表是否已全部选中（用于工具栏状态显示）。 */
  function isAllSelected(): boolean {
    const order = getOrderedIds()
    return order.length > 0 && order.every((id) => selected.value.has(id))
  }

  /** 选中集是否为空或部分选中（用于 indeterminate 状态）。 */
  function isPartial(): boolean {
    const order = getOrderedIds()
    return selected.value.size > 0 && !isAllSelected() && order.some((id) => selected.value.has(id))
  }

  /**
   * 收缩选择集：移除已不在当前显示列表中的 id。
   * 供"筛选变化后自动收缩"场景调用（替代此前散落在视图里的 watch 逻辑）。
   */
  function shrinkToVisible() {
    const visible = new Set(getOrderedIds())
    const next = new Set([...selected.value].filter((id) => visible.has(id)))
    if (next.size !== selected.value.size) replace(next)
  }

  return {
    selected: selected as Ref<Set<number>>,
    anchor,
    count,
    isEmpty,
    applySelection,
    selectRange,
    selectAll,
    clear,
    setSelection,
    addToSelection,
    removeFromSelection,
    toggle,
    isAllSelected,
    isPartial,
    shrinkToVisible,
  }
}

export type SelectionModel = ReturnType<typeof useSelection>
