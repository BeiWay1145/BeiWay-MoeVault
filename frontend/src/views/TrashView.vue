<script setup lang="ts">
import { ref } from 'vue'
import { ElMessageBox, ElMessage } from 'element-plus'

interface MockTrashItem {
  id: number
  name: string
  reason: string
  deletedAt: string
  hue: number
}

const items = ref<MockTrashItem[]>([
  { id: 1, name: '夏风_0031.png', reason: '冗余删除', deletedAt: '2024-06-12', hue: 20 },
  { id: 2, name: '海边_0017.png', reason: '手动删除', deletedAt: '2024-06-11', hue: 140 },
  { id: 3, name: '雨夜_0042.png', reason: '冗余删除', deletedAt: '2024-06-09', hue: 260 },
])

function restore(item: MockTrashItem) {
  ElMessage.success(`已恢复: ${item.name}（骨架占位）`)
  items.value = items.value.filter((i) => i.id !== item.id)
}

function purge(item: MockTrashItem) {
  ElMessageBox.confirm(`永久删除「${item.name}」？此操作不可恢复。`, '永久删除', { type: 'error' })
    .then(() => {
      ElMessage.success(`已永久删除: ${item.name}（骨架占位）`)
      items.value = items.value.filter((i) => i.id !== item.id)
    })
    .catch(() => {})
}

function purgeAll() {
  ElMessageBox.confirm(`清空回收站（${items.value.length} 项）？此操作不可恢复。`, '清空回收站', {
    type: 'error',
    confirmButtonText: '清空',
  })
    .then(() => {
      ElMessage.success('回收站已清空（骨架占位）')
      items.value = []
    })
    .catch(() => {})
}
</script>

<template>
  <div class="trash-page">
    <div class="toolbar">
      <span>回收站（{{ items.length }} 项）</span>
      <el-button type="danger" plain style="margin-left: auto" :disabled="items.length === 0" @click="purgeAll">
        清空回收站
      </el-button>
    </div>

    <el-table :data="items" style="width: 100%">
      <el-table-column label="图片" width="120">
        <template #default="{ row }">
          <div
            class="thumb"
            :style="`background: hsl(${row.hue} 65% 72%)`"
            :title="row.name"
          />
        </template>
      </el-table-column>
      <el-table-column prop="name" label="文件名" />
      <el-table-column prop="reason" label="删除原因" width="140" />
      <el-table-column prop="deletedAt" label="删除时间" width="140" />
      <el-table-column label="操作" width="200">
        <template #default="{ row }">
          <el-button size="small" type="primary" plain @click="restore(row)">恢复</el-button>
          <el-button size="small" type="danger" plain @click="purge(row)">永久删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <el-empty v-if="items.length === 0" description="回收站为空" />
  </div>
</template>

<style scoped>
.trash-page {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
}
.thumb {
  width: 72px;
  height: 54px;
  border-radius: 6px;
}
</style>
