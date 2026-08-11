<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Search } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, type ImageItem } from '@/stores/library'
import { post } from '@/api/client'
import ImageWall from '@/components/ImageWall.vue'
import ImagePreview from '@/components/ImagePreview.vue'
import { useRouter } from 'vue-router'

const router = useRouter()
const library = useLibraryStore()
const keyword = ref('')
const aestheticMin = ref<number | null>(null)
const source = ref('')
const onlyRedundant = ref(false)
const tagsInput = ref('')

// 预览
const previewVisible = ref(false)
const previewImage = ref<ImageItem | null>(null)
function openPreview(img: ImageItem) {
  previewImage.value = img
  previewVisible.value = true
}

async function onRecycle(img: ImageItem) {
  try {
    await ElMessageBox.confirm(`将「${img.name}」移入回收站？可随时恢复。`, '删除确认', {
      type: 'warning',
      confirmButtonText: '移入回收站',
    })
  } catch {
    return
  }
  try {
    await post(`/images/${img.id}/recycle`, { reason: 'manual' })
    ElMessage.success('已移入回收站')
    await library.fetchImages()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

onMounted(() => {
  library.fetchImages().catch((e: Error) => ElMessage.error(e.message))
})

async function doSearch() {
  await library
    .applyFilter({
      q: keyword.value || undefined,
      aestheticMin: aestheticMin.value ?? undefined,
      source: source.value || undefined,
      isRedundant: onlyRedundant.value ? true : undefined,
      tags: tagsInput.value
        ? tagsInput.value.split(',').map((s) => s.trim()).filter(Boolean)
        : undefined,
    })
    .catch((e: Error) => ElMessage.error(e.message))
}

async function clearSearch() {
  keyword.value = ''
  aestheticMin.value = null
  source.value = ''
  onlyRedundant.value = false
  tagsInput.value = ''
  library.clearFilter()
  await library.fetchImages().catch((e: Error) => ElMessage.error(e.message))
}
</script>

<template>
  <div class="search-page">
    <div class="search-bar">
      <el-input
        v-model="keyword"
        size="large"
        placeholder="搜索文件名关键字"
        :prefix-icon="Search"
        clearable
        @keyup.enter="doSearch"
      />
    </div>

    <div class="filter-panel">
      <el-form label-width="70px" inline>
        <el-form-item label="标签">
          <el-input v-model="tagsInput" placeholder="逗号分隔，如 1girl,blue_archive" style="width: 240px" clearable />
        </el-form-item>
        <el-form-item label="美学分≥">
          <el-input-number v-model="aestheticMin" :min="1" :max="5" :step="0.1" placeholder="不限" />
        </el-form-item>
        <el-form-item label="来源">
          <el-select v-model="source" clearable placeholder="不限" style="width: 130px">
            <el-option value="danbooru" label="danbooru" />
            <el-option value="gelbooru" label="gelbooru" />
            <el-option value="local" label="本地" />
          </el-select>
        </el-form-item>
        <el-form-item label="冗余候选">
          <el-switch v-model="onlyRedundant" />
        </el-form-item>
      </el-form>
      <div class="filter-actions">
        <el-button type="primary" @click="doSearch">搜索</el-button>
        <el-button @click="clearSearch">清空筛选</el-button>
      </div>
    </div>

    <div class="result-head">
      <span>共 <b class="num-mono">{{ library.total }}</b> 张</span>
      <span class="hint">按 标签/美学分/来源/冗余候选 组合筛选（后端排序）</span>
    </div>

    <div class="wall-container">
      <ImageWall
        :images="library.images"
        :view-mode="library.viewMode"
        :selected="library.selected"
        @click="(img: any) => router.push(`/library/${img.id}`)"
        @toggle-select="library.toggleSelect($event.id)"
        @preview="openPreview"
        @recycle="onRecycle"
      />
    </div>

    <!-- 大图预览 -->
    <ImagePreview v-model="previewVisible" :image="previewImage" />
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
