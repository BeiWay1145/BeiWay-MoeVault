<script setup lang="ts">
import { computed } from 'vue'
import type { ImageItem } from '@/stores/library'
import { thumbUrl } from '@/stores/library'

const props = defineProps<{
  image: ImageItem
  selected?: boolean
  listMode?: boolean
}>()

const emit = defineEmits<{
  click: [image: ImageItem]
  toggleSelect: [image: ImageItem]
  preview: [image: ImageItem]
  recycle: [image: ImageItem]
}>()

const src = computed(() => thumbUrl(props.image.thumbRel))

function fmtSize(bytes: number) {
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`
  return `${Math.round(bytes / 1024)} KB`
}
</script>

<template>
  <div
    class="image-card"
    :class="{ selected, 'list-mode': listMode }"
    @click="emit('click', image)"
  >
    <div class="thumb">
      <el-image
        v-if="src"
        :src="src"
        fit="cover"
        class="thumb-img"
        lazy
      >
        <template #error>
          <div class="thumb-fallback">无图</div>
        </template>
      </el-image>
      <div v-else class="thumb-fallback">无图</div>
      <span v-if="image.isRedundant" class="badge redundant" title="冗余候选（同组存在更清晰图）">⚠ 模糊</span>
      <span v-if="image.aesthetic" class="badge aesthetic" title="美学评分">⭐ {{ image.aesthetic.toFixed(1) }}</span>
      <div class="hover-actions">
        <el-button size="small" circle @click.stop="emit('toggleSelect', image)">
          {{ selected ? '✓' : '选' }}
        </el-button>
        <el-button size="small" circle @click.stop="emit('preview', image)">🔍</el-button>
        <el-button size="small" circle @click.stop="emit('recycle', image)">🗑</el-button>
      </div>
    </div>
    <div class="meta">
      <div class="name" :title="image.name">{{ image.name }}</div>
      <div class="sub">
        {{ image.width }}×{{ image.height }} · {{ fmtSize(image.sizeBytes) }}
        <span class="num-mono">清晰度 {{ image.clarity.toFixed(1) }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.image-card {
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
.hover-actions {
  position: absolute;
  inset: 0;
  display: none;
  align-items: center;
  justify-content: center;
  gap: 4px;
  background: rgba(0, 0, 0, 0.25);
}
.image-card:hover .hover-actions {
  display: flex;
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
