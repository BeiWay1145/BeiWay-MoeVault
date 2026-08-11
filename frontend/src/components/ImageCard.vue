<script setup lang="ts">
import { computed, ref } from 'vue'
import type { ImageItem } from '@/stores/library'
import { thumbUrl } from '@/stores/library'

const props = defineProps<{
  image: ImageItem
  selected?: boolean
  listMode?: boolean
  waterfallMode?: boolean
}>()

const emit = defineEmits<{
  click: [image: ImageItem]
  toggleSelect: [image: ImageItem]
  preview: [image: ImageItem]
  recycle: [image: ImageItem]
}>()

const src = computed(() => thumbUrl(props.image.thumbRel))

// 瀑布流：缩略图高度按原图宽高比（长图更高，形成错落）
const thumbStyle = computed(() => {
  if (!props.waterfallMode || !props.image.width || !props.image.height) return {}
  const ratio = props.image.height / props.image.width
  return { aspectRatio: `${props.image.width} / ${props.image.height}`, height: 'auto' }
})

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
    :class="{ selected, 'list-mode': listMode, 'waterfall-mode': waterfallMode }"
    @click="emit('click', image)"
  >
    <div class="thumb" :style="thumbStyle">
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
      <span v-if="image.isAi" class="badge ai" title="AI 生成">AI</span>
      <button
        class="delete-btn"
        :class="{ armed }"
        :title="armed ? '再点一次移入回收站（Shift+点击直接删除）' : '移入回收站'"
        @click.stop="onDeleteClick"
      >
        <span class="del-x">✕</span>
      </button>
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
