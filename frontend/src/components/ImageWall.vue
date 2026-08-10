<script setup lang="ts">
import type { ImageItem, ViewMode } from '@/stores/library'
import ImageCard from '@/components/ImageCard.vue'

defineProps<{
  images: ImageItem[]
  viewMode: ViewMode
  selected?: Set<number>
}>()

const emit = defineEmits<{
  click: [image: ImageItem]
  toggleSelect: [image: ImageItem]
  preview: [image: ImageItem]
  recycle: [image: ImageItem]
}>()

// 骨架阶段：网格/瀑布流均以 CSS grid 渲染（瀑布流后续按原比例实现多列布局）。
// TODO(perf): 万级列表接入 vue-virtual-scroller 虚拟滚动 + 缩略图懒加载。
</script>

<template>
  <div class="image-wall" :class="`view-${viewMode}`">
    <template v-if="viewMode === 'list'">
      <div v-for="img in images" :key="img.id" class="list-row">
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
    </template>
    <template v-else>
      <ImageCard
        v-for="img in images"
        :key="img.id"
        :image="img"
        :selected="selected?.has(img.id)"
        @click="emit('click', $event)"
        @toggle-select="emit('toggleSelect', $event)"
        @preview="emit('preview', $event)"
        @recycle="emit('recycle', $event)"
      />
    </template>
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
.view-waterfall {
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  align-items: start;
}
.view-list {
  grid-template-columns: 1fr;
  gap: 8px;
}
.list-row {
  width: 100%;
}
</style>
