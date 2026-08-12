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
}>()

const emit = defineEmits<{
  click: [image: ImageItem]
  toggleSelect: [image: ImageItem]
  preview: [image: ImageItem]
  recycle: [image: ImageItem]
}>()

// ---- 瀑布流行序错落布局 ----
// 原理：grid 多列 + 4px 行单元。每张卡片按测量高度算出 row span，
// 再按"严格行序"显式定位：第 i 张卡片放第 (i % N) 列，行起始为该列累计高度。
// → 阅读顺序严格从左到右、从上到下；各列独立堆叠形成错落（行尾参差）。
const containerRef = ref<HTMLElement | null>(null)
const COL_GAP = 12
const ROW_UNIT = 4
const BASE_COL_WIDTH = 220 // auto 模式列宽基准（与原 columns 一致）
const MAX_AUTO_COLS = 5

/** 当前列数（0=尚未测量，用默认 1） */
const cols = ref(0)
/** 测量中：grid-auto-rows 切回 auto，item 自然高度（避免 4px 行高压扁） */
const measuring = ref(true)
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
  if (newCols !== cols.value) {
    cols.value = newCols
    await nextTick()
  }
  // 进入测量态（grid-auto-rows: auto），强制 reflow 读取自然高度
  measuring.value = true
  await nextTick()
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
  measuring.value = false
}

// 列表变化（增删/筛选/排序/翻页）→ 重新布局（保持当前滚动位置，不打断浏览）
watch(
  () => props.images.map((i) => i.id).join(','),
  async () => {
    await nextTick()
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
let resizeTimer: number | undefined
onMounted(async () => {
  await nextTick()
  await layoutWaterfall()
  const el = containerRef.value
  if (el) {
    resizeObs = new ResizeObserver(() => {
      // 窗口宽度变化（即使列数不变）也会改变卡片宽度 → aspect-ratio 高度变化 → span 失效。
      // 因此任何宽度变化都需重新布局；防抖避免拖拽窗口时频繁重排。
      if (resizeTimer !== undefined) window.clearTimeout(resizeTimer)
      resizeTimer = window.setTimeout(() => {
        resizeTimer = undefined
        layoutWaterfall()
      }, 150)
    })
    resizeObs.observe(el)
  }
})
onBeforeUnmount(() => {
  resizeObs?.disconnect()
  resizeObs = null
  if (resizeTimer !== undefined) {
    window.clearTimeout(resizeTimer)
    resizeTimer = undefined
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
  if (measuring.value) return {}
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
      name="flip"
      tag="div"
      class="waterfall"
      :class="{ measuring }"
      :style="waterfallStyle"
    >
      <div
        v-for="img in images"
        :key="img.id"
        class="waterfall-item"
        :data-image-id="img.id"
        :style="itemStyle(img)"
      >
        <ImageCard
          :image="img"
          :selected="selected?.has(img.id)"
          waterfall-mode
          @click="emit('click', $event)"
          @toggle-select="emit('toggleSelect', $event)"
          @preview="emit('preview', $event)"
          @recycle="emit('recycle', $event)"
        />
      </div>
    </TransitionGroup>
  </div>

  <div v-else class="image-wall" :class="`view-${viewMode}`">
    <TransitionGroup v-if="viewMode === 'list'" name="flip" tag="div" class="list-wrap">
      <div v-for="img in images" :key="img.id" class="list-row" :data-image-id="img.id">
        <ImageCard
          :image="img"
          :selected="selected?.has(img.id)"
          list-mode
          @click="emit('click', $event)"
          @toggle-select="emit('toggleSelect', $event)"
          @preview="emit('preview', $event)"
          @recycle="emit('recycle', $event)"
        />
      </div>
    </TransitionGroup>
    <TransitionGroup v-else name="flip" tag="div" class="grid-wrap">
      <div v-for="img in images" :key="img.id" class="grid-cell" :data-image-id="img.id">
        <ImageCard
          :image="img"
          :selected="selected?.has(img.id)"
          @click="emit('click', $event)"
          @toggle-select="emit('toggleSelect', $event)"
          @preview="emit('preview', $event)"
          @recycle="emit('recycle', $event)"
        />
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.image-wall {
  display: grid;
  gap: 12px;
}
.view-grid {
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
}
.view-list {
  grid-template-columns: 1fr;
  gap: 8px;
}
.grid-wrap {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
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
.waterfall.measuring {
  grid-auto-rows: auto; /* 测量态：恢复自然高度读取 offsetHeight */
}
.waterfall-item {
  break-inside: avoid;
  margin-bottom: 12px;
  min-width: 0;
  overflow: hidden; /* 防止卡片内 nowrap 文字撑宽导致横向溢出 */
}

/* 删除/新增补位动效（瀑布流、网格、列表通用） */
.flip-enter-active,
.flip-leave-active,
.flip-move {
  transition: all 0.25s ease;
}
.flip-enter-from {
  opacity: 0;
  transform: scale(0.92);
}
.flip-leave-to {
  opacity: 0;
  transform: scale(0.92);
}
.flip-leave-active {
  position: absolute;
  z-index: 2;
}
</style>
