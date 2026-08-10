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
</script>

<template>
  <!-- 瀑布流：CSS columns 多列，卡片按自身比例高度（break-inside 避免截断） -->
  <div v-if="viewMode === 'waterfall'" class="waterfall">
    <div v-for="img in images" :key="img.id" class="waterfall-item">
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
  </div>

  <div v-else class="image-wall" :class="`view-${viewMode}`">
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
.view-list {
  grid-template-columns: 1fr;
  gap: 8px;
}
.list-row {
  width: 100%;
}

/* 瀑布流：多列 + 卡片按比例高度 */
.waterfall {
  columns: 5 220px;
  column-gap: 12px;
}
.waterfall-item {
  break-inside: avoid;
  margin-bottom: 12px;
}
</style>
