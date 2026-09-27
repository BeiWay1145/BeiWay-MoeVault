<script setup lang="ts">
import type { ImageItem, ViewMode } from '@/stores/library'
import ImageCard from '@/components/ImageCard.vue'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

const props = defineProps<{
  images: ImageItem[]
  viewMode: ViewMode
  selected?: Set<number>
  /** 瀑布流列数：auto=按容器宽度自适应（220px 基准，最多 5 列）/ 2-6=固定列数 */
  waterfallColumns?: string
  /**
   * 是否播放"渐进入场"动画（从上到下、从左到右依次淡入上浮）。
   * 仅在首次加载 / 切换搜索词条时为 true；
   * 滚动追加、窗口尺寸变化等场景为 false（保持即时显示，避免闪烁）。
   */
  appearAnim?: boolean
  /**
   * 动画批次号：每次需要播放入场动画时由父级递增。
   * 作为列表项 key 的前缀 → 元素被重建 → CSS 动画可靠重播
   * （仅改 class 常因浏览器优化而不重放，这是最稳的做法）。
   */
  appearEpoch?: number
}>()

const emit = defineEmits<{
  click: [image: ImageItem]
  toggleSelect: [image: ImageItem]
  preview: [image: ImageItem]
  recycle: [image: ImageItem]
  /** 资源管理器式选择（携带 ctrl/shift 修饰键）。 */
  select: [image: ImageItem, mods: { ctrl: boolean; shift: boolean }]
  /** 右键（父级决定作用范围）。 */
  contextmenu: [image: ImageItem, event: MouseEvent]
  /** 在已选图片上拖动 → 拖出到资源管理器。 */
  dragOut: [image: ImageItem]
}>()

// ---- 瀑布流行序错落布局 ----
// 原理：grid 多列 + 4px 行单元。每张卡片按测量高度算出 row span，
// 再按"严格行序"显式定位：第 i 张卡片放第 (i % N) 列，行起始为该列累计高度。
// → 阅读顺序严格从左到右、从上到下；各列独立堆叠形成错落（行尾参差）。
// 注意：容器 align-items: start，卡片高度不受 grid 轨道压缩，offsetHeight 始终是真实高度，
// 因此重排时无需切换 grid-auto-rows（避免布局塌缩闪烁）。
/**
 * 计算某张图的入场动画延迟（毫秒）。
 *
 * 目标效果：从上到下、从左到右依次出现——
 * 因此延迟 = 行号 × 行延迟 + 列号 × 列延迟；未开启动画时返回 undefined（不播放）。
 * 单张最大延迟做上限封顶，避免一次加载几百张时末尾等待过久。
 */
const APPEAR_ROW_MS = 34
const APPEAR_COL_MS = 14
const APPEAR_MAX_MS = 620
/**
 * 列表项 key：**只用 id**。
 *
 * 曾把 appearEpoch 拼进 key 以强制重播动画，但那会让整表 DOM 在每次播放动画时
 * 全部销毁重建（200+ 张图时明显卡顿，且元素重建期间瀑布流测量到 0 高度、多次重排）。
 * 现改为「不改 key」，仅通过 appearDelay 的变化驱动卡片的动画样式（见 ImageCard）。
 */
function imageKey(img: ImageItem): string {
  return String(img.id)
}

function appearDelayOf(img: ImageItem, idx: number): number | undefined {
  if (!props.appearAnim) return undefined
  // 网格/列表模式：按容器实际列数推算（网格是 CSS auto-fill，用宽度估算）
  // 瀑布流：用已测量的 cols（未就绪时即时推算，保证行号正确而非全为 0）
  const n = Math.max(1, cols.value || resolveColumns() || 1)
  const row = Math.floor(idx / n)
  const col = idx % n
  return Math.min(APPEAR_MAX_MS, row * APPEAR_ROW_MS + col * APPEAR_COL_MS)
}

/** 转发选择事件（模板中不能写 TS 类型标注，故在脚本里定义）。 */
function forwardSelect(img: ImageItem, mods: { ctrl: boolean; shift: boolean }) {
  emit('select', img, mods)
}
/** 转发右键事件。 */
function forwardContext(img: ImageItem, e: MouseEvent) {
  emit('contextmenu', img, e)
}
/** 转发拖出事件。 */
function forwardDragOut(img: ImageItem) {
  emit('dragOut', img)
}

const containerRef = ref<HTMLElement | null>(null)
const COL_GAP = 12
const ROW_UNIT = 4
const BASE_COL_WIDTH = 220 // auto 模式列宽基准（与原 columns 一致）
const MAX_AUTO_COLS = 5

/** 当前列数（0=尚未测量，用默认 1） */
const cols = ref(0)
/** 每张图片的定位：{col, rowStart, span}（grid 坐标 0 基） */
const layout = ref<Record<number, { col: number; rowStart: number; span: number }>>({})

function resolveColumns(): number {
  const c = props.waterfallColumns ?? 'auto'
  if (c !== 'auto' && ['2', '3', '4', '5', '6'].includes(c)) return Number(c)
  const el = containerRef.value
  if (!el) return 1
  return Math.min(
    MAX_AUTO_COLS,
    Math.max(1, Math.floor((el.clientWidth + COL_GAP) / (BASE_COL_WIDTH + COL_GAP))),
  )
}

/** 测量卡片自然高度 → 按严格行序计算每张卡片的行列定位。 */
async function layoutWaterfall() {
  const el = containerRef.value
  if (!el || props.viewMode !== 'waterfall' || props.images.length === 0) return
  const newCols = resolveColumns()
  const colsChanged = newCols !== cols.value
  if (colsChanged) {
    cols.value = newCols
  }
  // 等一帧：列数变化会改变 grid 模板，卡片宽度随之变化；
  // 宽度变化（如侧边栏收起/展开）同样需要让浏览器先完成布局，
  // 否则 offsetHeight 读到的是过渡中间态 → span 算错 → 间隙变大或卡片重叠（BUG2）。
  await nextTick()
  await new Promise<void>((r) => requestAnimationFrame(() => r()))
  const items = el.querySelectorAll<HTMLElement>('.waterfall-item')
  // 第一遍：测每张卡片高度 → row span。
  // 注意：offsetHeight 不含 margin-bottom，但 item 在 grid 轨道内的实际占位 = 卡片高 + 12px 间距，
  // 必须把间距计入 span，否则卡片高度恰为 4px 倍数时 margin 溢出轨道与下一张重叠。
  const spans: Record<number, number> = {}
  items.forEach((it) => {
    const id = Number(it.dataset.imageId)
    if (!Number.isFinite(id)) return
    const h = it.offsetHeight
    spans[id] = Math.max(1, Math.ceil((h + COL_GAP) / ROW_UNIT))
  })
  // 第二遍：严格行序分配列（第 i 张 → 列 i%N），每列独立堆叠（错落）
  const colHeights = new Array<number>(newCols).fill(0)
  const map: Record<number, { col: number; rowStart: number; span: number }> = {}
  props.images.forEach((img, idx) => {
    const col = idx % newCols
    const span = spans[img.id] ?? 1
    map[img.id] = { col, rowStart: colHeights[col], span }
    colHeights[col] += span
  })
  layout.value = map
}

// 列表变化（增删/筛选/排序/翻页）→ 重新布局（保持当前滚动位置，不打断浏览）
watch(
  () => props.images.map((i) => i.id).join(','),
  async () => {
    await nextTick()
    await layoutWaterfall()
  },
)
// 动画批次变化 → 元素被重建（key 变了）→ 瀑布流需重新测量布局。
// 注意：元素重建后卡片尚未完成首帧渲染，立即测量会得到 0 高度并触发多轮重排（卡顿）。
// 因此等两帧（nextTick + rAF）再测，且只测一次。
watch(
  () => props.appearEpoch,
  async () => {
    await nextTick()
    await new Promise<void>((r) => requestAnimationFrame(() => r()))
    await layoutWaterfall()
  },
)
// 列数设置变化 → 重新布局
watch(
  () => props.waterfallColumns,
  async () => {
    await nextTick()
    await layoutWaterfall()
  },
)
// 切到瀑布流视图 → 激活时重新布局
watch(
  () => props.viewMode,
  async (v) => {
    if (v === 'waterfall') {
      await nextTick()
      await layoutWaterfall()
    }
  },
)

let resizeObs: ResizeObserver | null = null
let rafId: number | undefined
/** 侧边栏过渡结束后的收尾定时器（见下）。 */
let settleTimer: number | undefined
/** 上一次测量到的容器宽度（用于识别宽度变化）。 */
let lastWidth = 0

/**
 * 重排瀑布流（合并到下一帧，避免密集回调导致抖动）。
 *
 * 什么时候需要重排：容器宽度变化会改变卡片宽度 → aspect-ratio 高度变化 →
 * 之前算出的 row span 失效，必须重新测量，否则出现间隙变大或卡片重叠（BUG2）。
 * 典型来源：窗口缩放、以及**侧边栏收起/展开**（宽度过渡动画）。
 */
function scheduleLayout() {
  if (rafId !== undefined) return
  rafId = requestAnimationFrame(() => {
    rafId = undefined
    layoutWaterfall()
  })
}

onMounted(async () => {
  await nextTick()
  await layoutWaterfall()
  const el = containerRef.value
  if (el) {
    lastWidth = el.clientWidth
    resizeObs = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? el.clientWidth
      // 宽度未变（例如仅内容高度变化）→ 无需重排，省掉无谓开销
      if (Math.abs(w - lastWidth) < 0.5) return
      lastWidth = w
      scheduleLayout()
      // 侧边栏是**过渡动画**：过渡期间宽度连续变化，最后一帧回调未必在过渡结束之后。
      // 这里再补一次延时重排，确保最终宽度下位置正确（修 BUG2：收起/展开后不重算）。
      if (settleTimer !== undefined) window.clearTimeout(settleTimer)
      settleTimer = window.setTimeout(() => scheduleLayout(), 320)
    })
    resizeObs.observe(el)
  }
})
onBeforeUnmount(() => {
  resizeObs?.disconnect()
  resizeObs = null
  if (rafId !== undefined) {
    cancelAnimationFrame(rafId)
    rafId = undefined
  }
  if (settleTimer !== undefined) {
    window.clearTimeout(settleTimer)
    settleTimer = undefined
  }
})

/** 瀑布流容器 style（grid-template-columns 由 cols 控制）。
 *  minmax(0, 1fr)：允许列收缩到内容最小宽度以下，避免卡片 nowrap 文字撑出横向滚动条。 */
const waterfallStyle = computed(() => {
  const c = Math.max(1, cols.value || resolveColumns())
  return { gridTemplateColumns: `repeat(${c}, minmax(0, 1fr))` }
})

/** 单张卡片的 grid 定位 style（0 基 → 1 基） */
function itemStyle(img: ImageItem) {
  const p = layout.value[img.id]
  if (!p) return {}
  return {
    gridColumnStart: p.col + 1,
    gridRowStart: p.rowStart + 1,
    gridRowEnd: p.rowStart + p.span + 1,
  }
}
</script>

<template>
  <!-- 瀑布流：行序错落（grid + 测量 row-span），DOM 顺序 = 从左到右、从上到下 -->
  <div v-if="viewMode === 'waterfall'" ref="containerRef" class="waterfall-measure-wrap">
    <TransitionGroup
      :name="appearAnim ? 'flip' : ''"
      tag="div"
      class="waterfall"
      :style="waterfallStyle"
    >
      <div
        v-for="(img, idx) in images"
        :key="imageKey(img)"
        class="waterfall-item"
        :data-image-id="img.id"
        :style="itemStyle(img)"
      >
        <ImageCard
          :image="img"
          :selected="selected?.has(img.id)"
          :appear-delay="appearDelayOf(img, idx)"
          :appear-epoch="appearEpoch"
          waterfall-mode
          @click="emit('click', $event)"
          @toggle-select="emit('toggleSelect', $event)"
          @preview="emit('preview', $event)"
          @recycle="emit('recycle', $event)"
          @select="forwardSelect"
          @contextmenu="forwardContext"
          @drag-out="forwardDragOut"
        />
      </div>
    </TransitionGroup>
  </div>

  <div v-else class="image-wall" :class="`view-${viewMode}`">
    <TransitionGroup v-if="viewMode === 'list'" name="flip" tag="div" class="list-wrap">
      <div v-for="(img, idx) in images" :key="imageKey(img)" class="list-row" :data-image-id="img.id">
        <ImageCard
          :image="img"
          :selected="selected?.has(img.id)"
          :appear-delay="appearDelayOf(img, idx)"
          :appear-epoch="appearEpoch"
          list-mode
          @click="emit('click', $event)"
          @toggle-select="emit('toggleSelect', $event)"
          @preview="emit('preview', $event)"
          @recycle="emit('recycle', $event)"
          @select="forwardSelect"
          @contextmenu="forwardContext"
          @drag-out="forwardDragOut"
        />
      </div>
    </TransitionGroup>
    <TransitionGroup v-else name="flip" tag="div" class="grid-wrap">
      <div v-for="(img, idx) in images" :key="imageKey(img)" class="grid-cell" :data-image-id="img.id">
        <ImageCard
          :image="img"
          :selected="selected?.has(img.id)"
          :appear-delay="appearDelayOf(img, idx)"
          :appear-epoch="appearEpoch"
          @click="emit('click', $event)"
          @toggle-select="emit('toggleSelect', $event)"
          @preview="emit('preview', $event)"
          @recycle="emit('recycle', $event)"
          @select="forwardSelect"
          @contextmenu="forwardContext"
          @drag-out="forwardDragOut"
        />
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
/* 外层容器用 block：内层 .grid-wrap / .list-wrap 各自负责多列布局。
   若外层是 grid 且只有一个子项，内层宽度会被约束为外层第一格（一列宽）→ 网格只显示靠左一列。 */
.image-wall {
  display: block;
}
.grid-wrap {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
  width: 100%;
}
.list-wrap {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.list-row {
  width: 100%;
}

/* 瀑布流：行序错落。grid 多列 + 4px 行单元，JS 显式定位（严格行序 + 列独立堆叠）。
   纵向间距用 item 的 margin-bottom 提供（row-gap 0，避免 gap 计入 span 计算）。 */
.waterfall-measure-wrap {
  width: 100%;
}
.waterfall {
  display: grid;
  grid-auto-rows: 4px;
  column-gap: 12px;
  row-gap: 0;
  align-items: start;
}
.waterfall-item {
  break-inside: avoid;
  margin-bottom: 12px;
  min-width: 0;
  overflow: hidden; /* 防止卡片内 nowrap 文字撑宽导致横向溢出 */
}

/* 删除/新增补位动效（瀑布流、网格、列表通用）。
   注意：这里**不能用 transition: all** —— 它会把 grid 定位属性、尺寸等一并做成动画，
   表现为卡片从旧位置"飞"到新位置（用户反馈的"从右下侧汇聚到左上角"）。
   只过渡透明度。 */
.flip-move {
  transition: opacity 0.25s ease;
}
.flip-enter-active,
.flip-leave-active {
  transition: opacity 0.2s ease;
}
.flip-enter-from,
.flip-leave-to {
  opacity: 0;
}
.flip-leave-active {
  position: absolute;
  z-index: 2;
}
</style>
