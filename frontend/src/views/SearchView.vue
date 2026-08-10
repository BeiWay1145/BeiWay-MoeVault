<script setup lang="ts">
import { ref } from 'vue'
import { Search } from '@element-plus/icons-vue'
import { useLibraryStore } from '@/stores/library'
import ImageWall from '@/components/ImageWall.vue'
import { useRouter } from 'vue-router'

const router = useRouter()
const library = useLibraryStore()
const keyword = ref('')
const aestheticMin = ref(3.0)
const dateRange = ref<[string, string] | null>(null)

// 骨架占位：关键字直接过滤 mock 名称，组合筛选器待接入后端
const filtered = () =>
  library.images.filter(
    (i) => !keyword.value || i.name.toLowerCase().includes(keyword.value.toLowerCase()),
  )
</script>

<template>
  <div class="search-page">
    <div class="search-bar">
      <el-input
        v-model="keyword"
        size="large"
        placeholder="搜索标签 / 文件名（FTS）"
        :prefix-icon="Search"
        clearable
      />
    </div>

    <div class="filter-panel">
      <el-form label-width="70px" inline>
        <el-form-item label="美学分">
          <el-slider v-model="aestheticMin" :min="1" :max="5" :step="0.1" style="width: 180px" show-input />
        </el-form-item>
        <el-form-item label="日期">
          <el-date-picker v-model="dateRange" type="daterange" start-placeholder="开始" end-placeholder="结束" />
        </el-form-item>
        <el-form-item label="来源">
          <el-checkbox-group>
            <el-checkbox value="danbooru" checked>danbooru</el-checkbox>
            <el-checkbox value="gelbooru">gelbooru</el-checkbox>
            <el-checkbox value="local">本地</el-checkbox>
          </el-checkbox-group>
        </el-form-item>
      </el-form>
      <div class="filter-actions">
        <el-button type="primary">保存为智能视图（骨架占位）</el-button>
        <el-button>清空筛选</el-button>
      </div>
    </div>

    <div class="result-head">
      <span>共 <b class="num-mono">{{ filtered().length }}</b> 张</span>
      <span class="hint">组合筛选器与 FTS 搜索将在接入后端后生效（docs/UI_DESIGN.md 4.4）</span>
    </div>

    <div class="wall-container">
      <ImageWall
        :images="filtered()"
        :view-mode="library.viewMode"
        @click="(img: any) => router.push(`/library/${img.id}`)"
      />
    </div>
  </div>
</template>

<style scoped>
.search-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  gap: 12px;
}
.search-bar {
  max-width: 640px;
}
.filter-panel {
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 8px;
  padding: 12px;
}
.filter-actions {
  margin-top: 4px;
}
.result-head {
  display: flex;
  align-items: baseline;
  gap: 12px;
}
.hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
</style>
