<script setup lang="ts">
import { ref } from 'vue'
import { Plus } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'

interface MockTag {
  id: number
  name: string
  nameCn: string
  category: string
  count: number
  blacklisted: boolean
}

const tags = ref<MockTag[]>([
  { id: 1, name: '1girl', nameCn: '女孩', category: 'general', count: 12304, blacklisted: false },
  { id: 2, name: 'original', nameCn: '原创', category: 'meta', count: 8911, blacklisted: false },
  { id: 3, name: 'landscape', nameCn: '风景', category: 'general', count: 4210, blacklisted: false },
  { id: 4, name: 'rating:explicit', nameCn: '', category: 'meta', count: 9302, blacklisted: true },
  { id: 5, name: '我的壁纸', nameCn: '', category: 'custom', count: 128, blacklisted: false },
])

const cnDict = ref(false)
const newName = ref('')

function createTag() {
  if (!newName.value.trim()) return
  tags.value.push({
    id: Date.now(),
    name: newName.value.trim(),
    nameCn: '',
    category: 'custom',
    count: 0,
    blacklisted: false,
  })
  newName.value = ''
  ElMessage.success('已创建自定义标签（骨架占位）')
}

function toggleBlacklist(t: MockTag) {
  t.blacklisted = !t.blacklisted
  ElMessage.success(`${t.name} 已${t.blacklisted ? '加入黑名单' : '移出黑名单'}（骨架占位）`)
}

function merge(t: MockTag) {
  ElMessage.info(`合并「${t.name}」到目标标签（骨架占位）`)
}
</script>

<template>
  <div class="tags-page">
    <div class="toolbar">
      <el-input v-model="newName" placeholder="新建自定义标签名" style="width: 200px" @keyup.enter="createTag" />
      <el-button type="primary" :icon="Plus" @click="createTag">新建</el-button>
      <div class="spacer" />
      <el-switch v-model="cnDict" active-text="中文字典" inactive-text="英文" />
      <el-button>导入 / 导出 CSV（骨架占位）</el-button>
    </div>

    <el-table :data="tags">
      <el-table-column prop="name" label="名称(EN)" />
      <el-table-column prop="nameCn" label="中文" width="140" />
      <el-table-column prop="category" label="分类" width="120" />
      <el-table-column prop="count" label="关联图数" width="120">
        <template #default="{ row }">
          <span class="num-mono">{{ row.count }}</span>
        </template>
      </el-table-column>
      <el-table-column label="黑名单" width="120">
        <template #default="{ row }">
          <el-tag :type="row.blacklisted ? 'danger' : 'info'" size="small">
            {{ row.blacklisted ? '已屏蔽' : '正常' }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="260">
        <template #default="{ row }">
          <el-button size="small" @click="merge(row)">合并</el-button>
          <el-button size="small">重命名</el-button>
          <el-button size="small" :type="row.blacklisted ? 'primary' : 'danger'" plain @click="toggleBlacklist(row)">
            {{ row.blacklisted ? '解除屏蔽' : '屏蔽' }}
          </el-button>
        </template>
      </el-table-column>
    </el-table>
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
