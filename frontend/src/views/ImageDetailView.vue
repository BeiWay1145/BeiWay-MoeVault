<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useLibraryStore, thumbUrl } from '@/stores/library'

const route = useRoute()
const library = useLibraryStore()

const image = computed(() => library.images.find((i) => i.id === Number(route.params.id)))
const newTag = ref('')
const star = ref(image.value?.aesthetic ?? 0)
</script>

<template>
  <div v-if="image" class="detail">
    <div class="viewer">
      <div class="stage">
        <el-image
          :src="thumbUrl(image.thumbRel)"
          fit="contain"
          class="stage-img"
        >
          <template #error>
            <span class="placeholder-name">{{ image.name }}</span>
          </template>
        </el-image>
      </div>
      <div class="viewer-toolbar">
        <el-button>◀ 上一张</el-button>
        <el-button>下一张 ▶</el-button>
        <el-button>全屏</el-button>
      </div>
      <!-- 相似图片：骨架占位 -->
      <div class="similar">
        <span class="similar-title">相似图片</span>
        <div class="similar-row">
          <div v-for="n in 6" :key="n" class="similar-thumb" :style="`background: hsl(${(n * 31) % 360} 60% 72%)`" />
        </div>
      </div>
    </div>

    <div class="panel">
      <el-descriptions :column="1" title="基本信息" border>
        <el-descriptions-item label="尺寸">{{ image.width }} × {{ image.height }}</el-descriptions-item>
        <el-descriptions-item label="清晰度">
          <span class="num-mono">{{ image.clarity.toFixed(1) }}</span>
        </el-descriptions-item>
        <el-descriptions-item label="美学分">
          <el-rate v-model="star" disabled :max="5" />
          <span class="num-mono">{{ image.aesthetic?.toFixed(1) ?? '—' }}</span>
        </el-descriptions-item>
        <el-descriptions-item label="导入时间">{{ new Date(image.importedAt * 1000).toLocaleDateString() }}</el-descriptions-item>
        <el-descriptions-item label="状态">
          <el-tag v-if="image.isRedundant" type="warning">冗余候选</el-tag>
          <el-tag v-else type="success">正常</el-tag>
        </el-descriptions-item>
      </el-descriptions>

      <div class="panel-block">
        <div class="panel-title">标签（骨架占位）</div>
        <el-tag v-for="n in 6" :key="n" class="tag" closable>{{ 'tag_' + n }}</el-tag>
        <el-input
          v-model="newTag"
          size="small"
          placeholder="添加标签"
          class="tag-input"
          @keyup.enter="newTag = ''"
        />
      </div>

      <div class="panel-block">
        <div class="panel-title">操作</div>
        <el-button type="danger" plain>入回收站</el-button>
        <el-button>导出</el-button>
        <el-button>生成 sidecar .txt</el-button>
      </div>
    </div>
  </div>
  <el-empty v-else description="图片不存在或已删除" />
</template>

<style scoped>
.detail {
  display: flex;
  gap: 16px;
  height: 100%;
}
.viewer {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.stage {
  flex: 1;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  min-height: 320px;
}
.placeholder-name {
  font-size: 15px;
  background: rgba(0, 0, 0, 0.35);
  padding: 6px 12px;
  border-radius: 6px;
}
.viewer-toolbar {
  display: flex;
  gap: 8px;
  justify-content: center;
}
.similar-title {
  font-size: 13px;
  color: var(--el-text-color-secondary);
}
.similar-row {
  display: flex;
  gap: 8px;
  margin-top: 6px;
  flex-wrap: wrap;
}
.similar-thumb {
  width: 72px;
  height: 54px;
  border-radius: 6px;
}
.panel {
  width: 360px;
  flex: none;
  overflow-y: auto;
}
.panel-block {
  margin-top: 16px;
}
.panel-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 8px;
}
.tag {
  margin: 0 6px 6px 0;
}
.tag-input {
  margin-top: 4px;
  width: 180px;
}
</style>
