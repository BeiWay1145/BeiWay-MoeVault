<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { ImageItem } from '@/stores/library'
import { thumbUrl } from '@/stores/library'

const props = defineProps<{
  image: ImageItem
  selected?: boolean
  listMode?: boolean
  waterfallMode?: boolean
  /**
   * 入场动画延迟（毫秒）。用于首次加载/切换筛选时按「从上到下、从左到右」渐变显示。
   * undefined = 不播放入场动画（滚动追加、窗口resize 等场景保持即时显示）。
   */
  appearDelay?: number
}>()

const emit = defineEmits<{
  click: [image: ImageItem]
  toggleSelect: [image: ImageItem]
  preview: [image: ImageItem]
  recycle: [image: ImageItem]
  /** 资源管理器式选择：携带修饰键（ctrl 叠加 / shift 范围）。
   *  缩略图区在按住 Ctrl 时也走此事件（按需求：Ctrl+点击图片同样选中）。 */
  select: [image: ImageItem, mods: { ctrl: boolean; shift: boolean }]
  /** 右键：ids 为菜单应作用的目标（由父级按"是否已选中"决定）。 */
  contextmenu: [image: ImageItem, event: MouseEvent]
  /** 已选中的图片上开始拖动 → 请求拖出到资源管理器（复制语义）。 */
  dragOut: [image: ImageItem, event: MouseEvent]
}>()

const src = computed(() => thumbUrl(props.image.thumbRel))

// ---- 入场动画 ----
//
// 设计（保持简单可靠）：
// - 父级传入 appearDelay（数值）= 本次要播放，延迟按"从上到下、从左到右"递增
// - 未传入（undefined）= 不播放，立即显示（翻页/追加/窗口 resize 等场景）
// - 动画通过内联 style 的 animationName 触发；配合父级按 epoch 重建元素，
//   每次需要播放时元素是新的 → 动画必然从头播放
//
// 注意：此前用「锁定式 playEnter」会导致动画状态永不复位，
// 翻页/追加项也带延迟（表现为"效果变奇怪"），已移除。
const appearStyle = computed(() => {
  if (props.appearDelay === undefined) return undefined
  return {
    animationDelay: `${props.appearDelay}ms`,
    animationFillMode: 'backwards',
    animationDuration: '0.32s',
    animationTimingFunction: 'ease-out',
    animationName: 'card-appear',
  }
})
const appearing = computed(() => props.appearDelay !== undefined)

/**
 * 缩略图自身的淡入。
 *
 * 为什么需要：卡片入场动画很快（0.34s），而缩略图是 lazy 加载的，
 * 常常在卡片动画结束后才解码完成——此时图片会"啪"地出现，
 * 破坏"渐显"的观感。这里让缩略图在加载完成时也做一次淡入（与卡片动画解耦）。
 */
const thumbLoaded = ref(false)
function onThumbLoad() {
  thumbLoaded.value = true
}
// 换图时重置（虚拟滚动/列表复用时同一 DOM 会换数据）
watch(
  () => props.image.id,
  () => {
    thumbLoaded.value = false
  },
)

// 瀑布流：缩略图高度按原图宽高比（长图更高，形成错落）
const thumbStyle = computed(() => {
  if (!props.waterfallMode || !props.image.width || !props.image.height) return {}
  const ratio = props.image.height / props.image.width
  return { aspectRatio: `${props.image.width} / ${props.image.height}`, height: 'auto' }
})

// ---- 资源管理器式点击语义 ----
// 需求：点击"名称/分辨率/大小/清晰度"这块边框区域 = 单选；
// 按住 Ctrl 时，点图片或边框都是叠加选择；单击缩略图（无修饰键）仍打开详情。

/** 缩略图区点击。Ctrl/Shift 时转为选择，否则打开详情。 */
function onThumbClick(e: MouseEvent) {
  if (e.ctrlKey || e.shiftKey) {
    emit('select', props.image, { ctrl: e.ctrlKey, shift: e.shiftKey })
    return
  }
  emit('click', props.image)
}

/** 信息区点击：始终走选择（无修饰键 = 替换式单选）。 */
function onMetaClick(e: MouseEvent) {
  e.stopPropagation()
  emit('select', props.image, { ctrl: e.ctrlKey, shift: e.shiftKey })
}

/** 右键：上报给父级决定作用范围，并阻止浏览器默认菜单。 */
function onContextMenu(e: MouseEvent) {
  e.preventDefault()
  emit('contextmenu', props.image, e)
}

// ---- 拖出到资源管理器 ----
// 需求：拖动已选图片 → 复制到桌面/资源管理器（无确认）。
// 浏览器无法用 HTML5 DnD 携带真实文件路径，因此交由壳层原生 DoDragDrop 处理。
// 这里只负责"检测到拖动意图"并通知父级。
const DRAG_OUT_THRESHOLD = 5
let pressX = 0
let pressY = 0
let pressOnSelected = false

function onThumbMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  // 阻止浏览器从这里开始文本选择：
  // 不阻止的话，Shift/Ctrl 连选（单击路径）会把卡片文字乃至整页文字一起选蓝。
  // 框选路径由 useMarqueeSelect 给 body 加 no-select 兜底。
  e.preventDefault()
  pressX = e.clientX
  pressY = e.clientY
  // 只有"已选中"的卡片才触发拖出（未选中时按下是框选/普通点击）
  pressOnSelected = props.selected === true
}

function onThumbMouseMove(e: MouseEvent) {
  if (!pressOnSelected) return
  if (e.buttons !== 1) return
  const dx = Math.abs(e.clientX - pressX)
  const dy = Math.abs(e.clientY - pressY)
  if (dx > DRAG_OUT_THRESHOLD || dy > DRAG_OUT_THRESHOLD) {
    pressOnSelected = false // 只触发一次
    emit('dragOut', props.image, e)
  }
}

function onThumbMouseUp() {
  pressOnSelected = false
}

// 右下角叉号两击删除：第一次点击变色（armed），再点一次送去回收站；Shift+点击直接删除
const armed = ref(false)
let armedTimer: number | undefined
function onDeleteClick(e: MouseEvent) {
  if (e.shiftKey) {
    armed.value = false
    emit('recycle', props.image)
    return
  }
  if (armed.value) {
    armed.value = false
    if (armedTimer !== undefined) window.clearTimeout(armedTimer)
    emit('recycle', props.image)
  } else {
    armed.value = true
    if (armedTimer !== undefined) window.clearTimeout(armedTimer)
    armedTimer = window.setTimeout(() => {
      armed.value = false
    }, 3000)
  }
}

function fmtSize(bytes: number) {
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`
  return `${Math.round(bytes / 1024)} KB`
}
</script>

<template>
  <div
    class="image-card"
    :class="{ selected, 'list-mode': listMode, 'waterfall-mode': waterfallMode, appearing }"
    :style="appearStyle"
    @contextmenu="onContextMenu"
  >
    <!-- 缩略图区：单击打开详情；Ctrl/Shift+单击 = 选择 -->
    <div
      class="thumb"
      :style="thumbStyle"
      @click="onThumbClick"
      @mousedown="onThumbMouseDown"
      @mousemove="onThumbMouseMove"
      @mouseup="onThumbMouseUp"
    >
      <el-image
        v-if="src"
        :src="src"
        fit="cover"
        class="thumb-img"
        :class="{ 'thumb-loaded': thumbLoaded }"
        lazy
        @load="onThumbLoad"
      >
        <template #error>
          <div class="thumb-fallback">无图</div>
        </template>
      </el-image>
      <div v-else class="thumb-fallback">无图</div>
      <span v-if="image.isRedundant" class="badge redundant" title="冗余候选（同组存在更清晰图）">⚠ 模糊</span>
      <span v-if="image.aesthetic" class="badge aesthetic" title="美学评分">⭐ {{ image.aesthetic.toFixed(1) }}</span>
      <span v-if="image.isAi" class="badge ai" title="AI 生成">AI</span>
      <span v-else class="badge not-ai" title="非 AI 生成">非AI</span>
      <button
        class="delete-btn"
        :class="{ armed }"
        :title="armed ? '再点一次移入回收站（Shift+点击直接删除）' : '移入回收站'"
        @click.stop="onDeleteClick"
      >
        <span class="del-x">✕</span>
      </button>
    </div>
    <!-- 信息区（名称/分辨率/大小/清晰度）：点击 = 单选；Ctrl/Shift 修饰 -->
    <div class="meta" @click="onMetaClick" @mousedown.prevent>
      <div class="name" :title="image.name">{{ image.name }}</div>
      <div class="sub">
        {{ image.width }}×{{ image.height }} · {{ fmtSize(image.sizeBytes) }}
        <span class="num-mono">清晰度 {{ image.clarity.toFixed(1) }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 入场：淡入 + 轻微上浮（渐变显示）。
   注意：动画由内联 style 的 animationName 触发（见 appearStyle），
   而不是靠切换 class —— 这样即使父级随后关掉 appearAnim，已开始的动画也不会被打断。 */
@keyframes card-appear {
  from {
    opacity: 0;
    transform: translateY(12px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

/* 缩略图懒加载完成时淡入：与卡片动画解耦，避免"图片晚到"导致突现。 */
.thumb-img {
  opacity: 0;
  transition: opacity 0.28s ease-out;
}
.thumb-img.thumb-loaded {
  opacity: 1;
}

@media (prefers-reduced-motion: reduce) {
  .image-card,
  .thumb-img {
    animation: none !important;
    transition: none !important;
    opacity: 1 !important;
  }
}

.image-card {
  /* 卡片内的文字（文件名/分辨率等）是信息展示，不是可选中内容；
     禁用后 Shift/Ctrl 连选不会把文字选蓝。 */
  user-select: none;
  border-radius: 8px;
  overflow: hidden;
  border: 2px solid transparent;
  background: var(--el-bg-color);
  box-shadow: var(--el-box-shadow-lighter);
  cursor: pointer;
  transition: border-color 0.15s;
}
.image-card.selected {
  border-color: var(--el-color-primary);
}
.thumb {
  position: relative;
  aspect-ratio: 4 / 3;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  background: var(--el-fill-color-light);
}
.thumb-img {
  width: 100%;
  height: 100%;
}
.thumb-fallback {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color-light);
  font-size: 12px;
}
.list-mode .thumb {
  aspect-ratio: auto;
  width: 96px;
  height: 72px;
  flex: none;
}
.waterfall-mode .thumb {
  aspect-ratio: auto;
  height: auto;
  width: 100%;
}
.waterfall-mode .thumb-img {
  display: block;
}
.badge {
  position: absolute;
  top: 6px;
  padding: 1px 6px;
  border-radius: 4px;
  font-size: 11px;
  color: #fff;
}
.badge.redundant {
  left: 6px;
  background: rgba(230, 162, 60, 0.9);
}
.badge.aesthetic {
  right: 6px;
  background: rgba(0, 0, 0, 0.45);
}
.badge.ai {
  left: 6px;
  bottom: 6px;
  top: auto;
  background: rgba(103, 194, 58, 0.9);
}
.badge.not-ai {
  left: 6px;
  bottom: 6px;
  top: auto;
  background: rgba(144, 147, 153, 0.85);
}
/* 右下角 32px 半透明圆底叉号（增强2）：单击变色待确认，再点删除；Shift+点击直接删除 */
.delete-btn {
  position: absolute;
  right: 8px;
  bottom: 8px;
  z-index: 5;
  width: 32px;
  height: 32px;
  border: none;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s, transform 0.15s;
  opacity: 0.85;
}
.delete-btn:hover {
  background: rgba(0, 0, 0, 0.7);
  opacity: 1;
}
.delete-btn.armed {
  background: rgba(230, 80, 80, 0.92);
  transform: scale(1.15);
  opacity: 1;
}
.del-x {
  font-size: 14px;
  line-height: 1;
}
.list-mode .delete-btn {
  width: 24px;
  height: 24px;
  right: 4px;
  bottom: 4px;
}
.list-mode .del-x {
  font-size: 11px;
}
.meta {
  padding: 6px 8px;
}
.name {
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.sub {
  font-size: 11px;
  color: var(--el-text-color-secondary);
  display: flex;
  justify-content: space-between;
  gap: 8px;
}
</style>
