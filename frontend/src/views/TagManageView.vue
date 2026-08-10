<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Plus } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { get } from '@/api/client'

interface TagItem {
  id: number
  name: string
  name_cn: string | null
  category: string
  is_custom: boolean
  is_blacklisted: boolean
  image_count: number
}

const tags = ref<TagItem[]>([])
const loading = ref(false)
const cnDict = ref(false)
const newName = ref('')

async function loadTags() {
  loading.value = true
  try {
    const d = await get<{ items: TagItem[]; total: number }>('/tags')
    tags.value = d.items
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

function createTag() {
  if (!newName.value.trim()) return
  ElMessage.info('自定义标签创建将在标签管理 API 完善后生效')
  newName.value = ''
}

function toggleBlacklist(t: TagItem) {
  ElMessage.info(`黑名单管理（${t.name}）将在后续实现`)
}

function merge(t: TagItem) {
  ElMessage.info(`合并「${t.name}」将在标签管理 API 完善后实现`)
}

onMounted(loadTags)
</script>

<template>
  <div class="tags-page">
    <div class="toolbar">
      <el-input v-model="newName" placeholder="新建自定义标签名" style="width: 200px" @keyup.enter="createTag" />
      <el-button type="primary" :icon="Plus" @click="createTag">新建</el-button>
      <div class="spacer" />
      <el-switch v-model="cnDict" active-text="中文字典" inactive-text="英文" />
    </div>

    <div v-loading="loading">
      <el-table v-if="tags.length > 0" :data="tags">
        <el-table-column prop="name" label="名称(EN)" />
        <el-table-column prop="name_cn" label="中文" width="140" />
        <el-table-column prop="category" label="分类" width="120" />
        <el-table-column prop="image_count" label="关联图数" width="120">
          <template #default="{ row }">
            <span class="num-mono">{{ row.image_count }}</span>
          </template>
        </el-table-column>
        <el-table-column label="黑名单" width="120">
          <template #default="{ row }">
            <el-tag :type="row.is_blacklisted ? 'danger' : 'info'" size="small">
              {{ row.is_blacklisted ? '已屏蔽' : '正常' }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="260">
          <template #default="{ row }">
            <el-button size="small" @click="merge(row)">合并</el-button>
            <el-button size="small">重命名</el-button>
            <el-button size="small" :type="row.is_blacklisted ? 'primary' : 'danger'" plain @click="toggleBlacklist(row)">
              {{ row.is_blacklisted ? '解除屏蔽' : '屏蔽' }}
            </el-button>
          </template>
        </el-table-column>
      </el-table>
      <el-empty v-else-if="!loading" description="暂无标签，导入图片并打标后自动出现" />
    </div>
  </div>
</template>

<style scoped>
.tags-page {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
}
.spacer {
  flex: 1;
}
</style>
