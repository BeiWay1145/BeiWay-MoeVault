<script setup lang="ts">
import { computed } from 'vue'
import { thumbUrl, type ImageItem } from '@/stores/library'

const props = defineProps<{
  image: ImageItem | null
}>()

const visible = defineModel<boolean>({ default: false })

const src = computed(() => (props.image ? thumbUrl(props.image.thumbRel) : undefined))
</script>

<template>
  <el-dialog
    v-model="visible"
    :title="image?.name ?? ''"
    width="70%"
    top="4vh"
    destroy-on-close
  >
    <div v-if="image" class="preview-body">
      <el-image :src="src" fit="contain" class="preview-img" :preview-src-list="src ? [src] : []">
        <template #error>
          <div class="preview-fallback">图片加载失败</div>
        </template>
      </el-image>
      <div class="preview-meta">
        <span>{{ image.width }}×{{ image.height }}</span>
        <span>清晰度 {{ image.clarity.toFixed(2) }}</span>
        <span v-if="image.aesthetic">美学 {{ image.aesthetic.toFixed(2) }}</span>
        <el-tag v-if="image.isRedundant" type="warning" size="small">冗余候选</el-tag>
      </div>
    </div>
  </el-dialog>
</template>

<style scoped>
.preview-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.preview-img {
  width: 100%;
  max-height: 70vh;
}
.preview-fallback {
  height: 40vh;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-secondary);
}
.preview-meta {
  display: flex;
  gap: 16px;
  color: var(--el-text-color-secondary);
  font-size: 13px;
}
</style>
