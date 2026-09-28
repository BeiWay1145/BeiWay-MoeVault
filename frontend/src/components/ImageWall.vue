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

/**
 * 缩略图加载完成。
 *
 * 这里**不再触发重排**：row span 现已由图片宽高比直接算出（见 layoutWaterfall），
 * 与缩略图是否加载完成无关，因此无需重排。
 * 保留监听仅为将来需要（例如图片实际比例与元数据不符）时可在此补一次 scheduleLayout。
 */
function onThumbReady() {
  // 故意留空：高度不再依赖图片加载状态
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

/**
 * 测量卡片自然高度 → 按严格行序计算每张卡片的行列定位。
 *
 * 两条必须遵守的规则（都踩过坑）：
 *
 * 1. **同步测量**：中间不能 await 让出线程。
 *    让出后浏览器会先按旧 layout 重排，随后测到的是中间态高度。
 *
 * 2. **测量前绝不能清空 layout**：
 *    清空会让所有卡片瞬间失去 grid 定位、挤进自动流（高度变得极小），
 *    此时测出来的 span 全是 1 → 布局永久塌陷（用户反馈的"全部堆叠"）。
 *    正确做法是「先在现有定位下测量，再整体替换 layout」（本函数即如此）。
 *
 * 调用方负责确保 DOM 已就绪（先 nextTick）。
 */
function layoutWaterfall() {
  const el = containerRef.value
  if (!el || props.viewMode !== 'waterfall' || props.images.length === 0) return
  // 容器尚未获得有效宽度（如 keep-alive 激活瞬间仍是 display:none）时不要计算：
  // 否则会用错误宽度（0 或过小）算出偏窄的卡片 → 卡片变高 → 内容总高偏大，
  // 返回图库后整体布局与离开时不一致（表现为"越往下偏移越大"）。
  // 此时直接返回，等 ResizeObserver 感知到真实宽度后会再触发一次。
  if (el.clientWidth < 50) return
  const newCols = resolveColumns()
  if (newCols !== cols.value) {
    cols.value = newCols
  }
  // 卡片宽度：由容器宽度与列数确定（比逐张测量 DOM 更可靠、也更快）
  const cardW = (el.clientWidth - COL_GAP * (newCols - 1)) / newCols

  // 第一遍：算每张卡片的 row span。
  //
  // **改为由图片宽高比推算，而不是测量 DOM 高度**：
  // 瀑布流卡片的高度 = 缩略图高度(aspect-ratio = 原图宽高比) + 信息区固定高度，
  // 这两者都可从数据直接得出，无需等 DOM 完成布局。
  // 用测量则必须保证"测量时卡片已有正确宽度"，一旦顺序有误就会读到塌陷高度，
  // 且缩略图 lazy 加载也会让高度变化——这是此前反复出现布局塌陷的根源。
  // 卡片信息区高度：与 ImageCard.vue 的 .meta 样式保持一致
  //   padding 6px×2 + .name(12px 行高约 17) + .sub(11px 行高约 16) ≈ 45
  // 留少量余量避免因字体差异导致重叠。
  const INFO_BAR_H = 48
  const spans: Record<number, number> = {}
  props.images.forEach((img) => {
    const ratio = img.width > 0 ? img.height / img.width : 1.4
    const h = cardW * ratio + INFO_BAR_H
    spans[img.id] = Math.max(1, Math.ceil((h + COL_GAP) / ROW_UNIT))
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
    layoutWaterfall()
  },
)
// 动画批次变化 → 元素被重建（key 变了）→ 瀑布流需重新测量布局。
// 注意：元素重建后卡片尚未完成首帧渲染，立即测量会得到 0 高度并触发多轮重排（卡顿）。
// 因此等两帧（nextTick + rAF）再测，且只测一次。
watch(
  () => props.appearEpoch,
  async () => {
    await nextTick()
    layoutWaterfall()
  },
)
// 列数设置变化 → 重新布局
watch(
  () => props.waterfallColumns,
  async () => {
    await nextTick()
    layoutWaterfall()
  },
)
// 切到瀑布流视图 → 激活时重新布局（列数在其它视图下未测量，需重新算）
watch(
  () => props.viewMode,
  async (v) => {
    if (v === 'waterfall') {
      await nextTick()
      layoutWaterfall()
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
  layoutWaterfall()
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

/**
 * 单张卡片的 grid 定位 style（0 基 → 1 基）。
 *
 * 关键：**必须有兜底定位**。
 * 若 layout 尚未算出（首帧、刚切到瀑布流、数据刚更新）就返回空对象，
 * 卡片会全部落进 grid 自动流的同一列，被挤成细条堆叠——
 * 这正是用户反复反馈的"布局塌陷"。
 * 兜底策略：按索引推算出列号与估算行高（用图片宽高比预估，无需等图片加载），
 * 让首帧就有合理位置；真实测量随后会覆盖它。
 */
function itemStyle(img: ImageItem, idx: number) {
  const p = layout.value[img.id]
  if (p) {
    return {
      gridColumnStart: p.col + 1,
      gridRowStart: p.rowStart + 1,
      gridRowEnd: p.rowStart + p.span + 1,
    }
  }
  // 兜底：按索引分列，行跨度按图片宽高比估算（4px 行单元 → span）
  const c = Math.max(1, cols.value || resolveColumns() || 1)
  const col = idx % c
  const rowIndex = Math.floor(idx / c)
  const ratio = img.width > 0 ? img.height / img.width : 1.4
  // 卡片宽度按容器宽度/列数估算，避免依赖尚未完成的布局
  const el = containerRef.value
  const cardW = el ? (el.clientWidth - COL_GAP * (c - 1)) / c : BASE_COL_WIDTH
  const estH = cardW * ratio + 48 // +48 预留信息区高度（与 layoutWaterfall 的 INFO_BAR_H 一致）
  const span = Math.max(1, Math.ceil((estH + COL_GAP) / ROW_UNIT))
  const rowStart = rowIndex * span
  return {
    gridColumnStart: col + 1,
    gridRowStart: rowStart + 1,
    gridRowEnd: rowStart + span + 1,
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
        :style="itemStyle(img, idx)"
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
          @thumb-ready="onThumbReady"
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
          @thumb-ready="onThumbReady"
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
          @thumb-ready="onThumbReady"
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
