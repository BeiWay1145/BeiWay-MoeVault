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
  <!-- 瀑布流：grid 多列，按行排序（从左到右、从上到下） -->
  <TransitionGroup v-if="viewMode === 'waterfall'" name="flip" tag="div" class="waterfall">
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

/* 瀑布流：grid 多列按行排序（从左到右、从上到下），卡片按自身比例高度 */
.waterfall {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
}
.waterfall-item {
  margin-bottom: 0;
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
