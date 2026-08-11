<script setup lang="ts">
import type { ImageItem, ViewMode } from '@/stores/library'
import ImageCard from '@/components/ImageCard.vue'
import { computed } from 'vue'

const props = defineProps<{
  images: ImageItem[]
  viewMode: ViewMode
  selected?: Set<number>
  /** 瀑布流列数：auto=传统瀑布流（columns 紧密错落）/ 2-6=固定列网格按行 */
  waterfallColumns?: string
}>()

// auto → 传统瀑布流 class；固定数字 → 网格列数 class
const waterfallClass = computed(() => {
  const c = props.waterfallColumns ?? 'auto'
  if (c === 'auto' || !['2', '3', '4', '5', '6'].includes(c)) return 'waterfall-auto'
  return `waterfall-grid cols-${c}`
})

const emit = defineEmits<{
  click: [image: ImageItem]
  toggleSelect: [image: ImageItem]
  preview: [image: ImageItem]
  recycle: [image: ImageItem]
}>()
</script>

<template>
  <!-- 瀑布流：auto=传统（columns 紧密错落）/ 固定列=网格按行 -->
  <TransitionGroup v-if="viewMode === 'waterfall'" name="flip" tag="div" :class="['waterfall', waterfallClass]">
    <div v-for="img in images" :key="img.id" class="waterfall-item" :data-image-id="img.id">
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

/* 瀑布流：auto=传统 columns（紧密错落，无空隙）；固定列=grid 按行（无空隙但等高） */
.waterfall-auto {
  columns: 5 220px;
  column-gap: 12px;
}
.waterfall-auto .waterfall-item {
  break-inside: avoid;
  margin-bottom: 12px;
}
.waterfall-grid {
  display: grid;
  gap: 12px;
}
.waterfall-grid.cols-2 {
  grid-template-columns: repeat(2, 1fr);
}
.waterfall-grid.cols-3 {
  grid-template-columns: repeat(3, 1fr);
}
.waterfall-grid.cols-4 {
  grid-template-columns: repeat(4, 1fr);
}
.waterfall-grid.cols-5 {
  grid-template-columns: repeat(5, 1fr);
}
.waterfall-grid.cols-6 {
  grid-template-columns: repeat(6, 1fr);
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
