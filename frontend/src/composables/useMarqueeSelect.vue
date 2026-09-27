<script setup lang="ts">
/**
 * 框选（橡皮筋选择）。
 *
 * 行为（按需求）：
 * - **只在空白区按下**才起框（图片上按下不触发，避免与"拖动图片"冲突）
 * - 拖动绘制半透明选择框；松开后选中与框相交的图片
 * - 按住 Ctrl 框选 = 叠加到现有选择（不按住 = 替换）
 * - 拖动距离 < 4px 视为普通点击，不触发
 *
 * 命中检测用 getBoundingClientRect()，因此天然兼容网格/瀑布流/列表三种布局
 * （瀑布流用 transform 定位，rect 仍返回真实视口坐标）。
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'

const props = defineProps<{
  /** 选择框的作用容器（其内的 [data-image-id] 元素参与命中检测）。 */
  containerRef: HTMLElement | null
  /** 命中后回调：ids 为被框住的图片，additive 表示是否叠加。 */
  onSelect: (ids: number[], additive: boolean) => void
  /** 空白处"普通单击"（未拖动）时回调：用于取消选择。 */
  onBlankClick?: () => void
  /**
   * 是否启用框选。
   *
   * 必须由父级控制：图库被 keep-alive 缓存，切到详情页时组件**不会卸载**，
   * 全局 mousedown 监听若继续生效，就会在详情页误触发框选、
   * 并因 no-select 导致详情页文字无法选中（BUG1）。
   */
  enabled?: boolean
}>()

/** 拖拽阈值：小于该像素视为点击。 */
const DRAG_THRESHOLD = 4

const active = ref(false)
const startX = ref(0)
const startY = ref(0)
const curX = ref(0)
const curY = ref(0)
/** 是否已超过阈值（决定是否绘制）。 */
const dragging = ref(false)

/** 选择框的样式（视口坐标，fixed 定位）。 */
function rectStyle() {
  const left = Math.min(startX.value, curX.value)
  const top = Math.min(startY.value, curY.value)
  const width = Math.abs(curX.value - startX.value)
  const height = Math.abs(curY.value - startY.value)
  return { left: left + 'px', top: top + 'px', width: width + 'px', height: height + 'px' }
}

/**
 * 判断是否应当起框。
 *
 * 条件：
 * 1. 没有落在图片卡片上（卡片上的按下是选择/拖出）
 * 2. 没有落在交互控件上（输入框/按钮/下拉/链接/菜单）
 * 3. 位于内容区域内——用"是否在 .app-main 内"判定，而非严格限定在图片墙容器：
 *    图片墙容器的左右留白、上下间距也属于可框选的空白区，
 *    此前用 containerRef.contains 会把这些区域排除掉（表现为"某些空白处拖不出框"）。
 */
function isBlankTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null
  if (!el) return true
  if (el.closest('[data-image-id]')) return false
  // 交互控件上不起框（避免与输入/点击冲突）
  if (el.closest('input, textarea, select, button, a, .el-select, .el-input, .el-checkbox, .el-dropdown, .ctx-menu, .el-dialog, .el-tabs, .side-nav, aside')) {
    return false
  }
  return true
}

function onMouseDown(e: MouseEvent) {
  // 未启用（如在详情页、或组件被 keep-alive 缓存但当前不在前台）时不响应
  if (props.enabled === false) return
  if (e.button !== 0) return
  if (!isBlankTarget(e.target)) return
  // 必须落在主内容区（排除左侧导航、顶栏、弹窗等）
  const el = props.containerRef
  const target = e.target as HTMLElement | null
  if (!target) return
  const inMain = !!target.closest('.app-main')
  if (!inMain && !(el && el.contains(target))) return
  active.value = true
  dragging.value = false
  startX.value = e.clientX
  startY.value = e.clientY
  curX.value = e.clientX
  curY.value = e.clientY
  // 框选期间禁止文本选择（只作用于图片墙容器，不用 body——
  // 加到 body 会波及详情页等其它区域）。
  el?.classList.add('no-select')
}

function onMouseMove(e: MouseEvent) {
  if (!active.value) return
  curX.value = e.clientX
  curY.value = e.clientY
  if (!dragging.value) {
    const dx = Math.abs(curX.value - startX.value)
    const dy = Math.abs(curY.value - startY.value)
    if (dx > DRAG_THRESHOLD || dy > DRAG_THRESHOLD) dragging.value = true
  }
  if (dragging.value) e.preventDefault()
}

function onMouseUp(e: MouseEvent) {
  if (!active.value) return
  const wasDragging = dragging.value
  active.value = false
  dragging.value = false
  props.containerRef?.classList.remove('no-select')
  if (!wasDragging) {
    // 空白处普通单击（未拖动）→ 取消选择（按需求：选中状态下点击空白取消）
    props.onBlankClick?.()
    return
  }
  // 计算命中：与选择框相交的图片卡片
  const box = {
    left: Math.min(startX.value, curX.value),
    top: Math.min(startY.value, curY.value),
    right: Math.max(startX.value, curX.value),
    bottom: Math.max(startY.value, curY.value),
  }
  const el = props.containerRef
  if (!el) return
  const ids: number[] = []
  el.querySelectorAll<HTMLElement>('[data-image-id]').forEach((node) => {
    const r = node.getBoundingClientRect()
    const hit = !(r.right < box.left || r.left > box.right || r.bottom < box.top || r.top > box.bottom)
    if (hit) {
      const id = Number(node.dataset.imageId)
      if (Number.isFinite(id)) ids.push(id)
    }
  })
  props.onSelect(ids, e.ctrlKey || e.metaKey)
}

onMounted(() => {
  document.addEventListener('mousedown', onMouseDown)
  document.addEventListener('mousemove', onMouseMove)
  document.addEventListener('mouseup', onMouseUp)
})
onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onMouseDown)
  document.removeEventListener('mousemove', onMouseMove)
  document.removeEventListener('mouseup', onMouseUp)
  props.containerRef?.classList.remove('no-select')
})
</script>

<template>
  <Teleport to="body">
    <div v-if="dragging" class="marquee" :style="rectStyle()" />
  </Teleport>
</template>

<style>
/* 框选期间禁止文本选择。作用于图片墙容器（而非 body），
   避免波及其它页面（如详情页本该可选中的文字）。 */
.no-select,
.no-select * {
  user-select: none !important;
  -webkit-user-select: none !important;
}
</style>

<style scoped>
.marquee {
  position: fixed;
  z-index: 3500;
  border: 1px solid var(--el-color-primary);
  background: color-mix(in srgb, var(--el-color-primary) 16%, transparent);
  border-radius: 2px;
  pointer-events: none;
}
</style>
