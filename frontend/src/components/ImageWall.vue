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
// 原理：grid 多列 + 固定 4px 行单元，每张卡片按其测量高度设置 grid-row-end: span N。
// 卡片按 DOM 顺序由 grid 自动放置 → 视觉顺序从左到右、从上到下（行序）；
// 各列卡片高度参差 → 保持瀑布流错落（行尾不齐，符合使用习惯）。
const containerRef = ref<HTMLElement | null>(null)
const COL_GAP = 12
const ROW_UNIT = 4
const BASE_COL_WIDTH = 220 // auto 模式列宽基准（与原 columns 一致）
const MAX_AUTO_COLS = 5

/** 当前列数（0=尚未测量，用默认 1） */
const cols = ref(0)
/** 每张图片的 grid-row span 映射 */
const spans = ref<Record<number, number>>({})
/** 测量中：grid-auto-rows 切回 auto，item 自然高度（避免 4px 行高压扁） */
const measuring = ref(true)

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

/** 测量每张卡片自然高度 → 计算 row span；列数变化时先更新列数再测。 */
async function measure() {
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
  const map: Record<number, number> = {}
  items.forEach((it) => {
    const id = Number(it.dataset.imageId)
    if (!Number.isFinite(id)) return
    const h = it.offsetHeight
    map[id] = Math.max(1, Math.ceil(h / ROW_UNIT))
  })
  spans.value = map
  measuring.value = false
}

// 列表变化（增删/筛选/翻页）→ 重测
watch(
  () => props.images.map((i) => i.id).join(','),
  async () => {
    await nextTick()
    await measure()
  },
)
// 列数设置变化 → 重测
watch(
  () => props.waterfallColumns,
  async () => {
    await nextTick()
    await measure()
  },
)
// 切到瀑布流视图 → 激活时重测
watch(
  () => props.viewMode,
  async (v) => {
    if (v === 'waterfall') {
      await nextTick()
      await measure()
    }
  },
)

let resizeObs: ResizeObserver | null = null
onMounted(async () => {
  await nextTick()
  await measure()
  const el = containerRef.value
  if (el) {
    resizeObs = new ResizeObserver(() => {
      // 列数变化才重测（宽度小变化不重排）
      const c = resolveColumns()
      if (c !== cols.value) measure()
    })
    resizeObs.observe(el)
  }
})
onBeforeUnmount(() => {
  resizeObs?.disconnect()
  resizeObs = null
})

/** 瀑布流容器 class + style（grid-template-columns 由 cols 控制） */
const waterfallStyle = computed(() => {
  const c = Math.max(1, cols.value || resolveColumns())
  return { gridTemplateColumns: `repeat(${c}, 1fr)` }
})
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
        :style="{ gridRowEnd: measuring ? 'auto' : `span ${spans[img.id] ?? 1}` }"
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

/* 瀑布流：行序错落。grid 多列 + 4px 行单元，卡片按测量高度跨行（各列参差） */
.waterfall-measure-wrap {
  width: 100%;
}
.waterfall {
  display: grid;
  grid-auto-rows: 4px;
  gap: 12px;
  align-items: start;
}
.waterfall.measuring {
  grid-auto-rows: auto; /* 测量态：恢复自然高度读取 offsetHeight */
}
.waterfall-item {
  break-inside: avoid;
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
