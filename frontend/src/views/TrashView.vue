<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { get, post } from '@/api/client'
import { thumbUrl } from '@/stores/dedup'

interface TrashItem {
  image_id: number
  rel_path: string
  thumb_rel: string
  reason: string
  original_rel: string
  deleted_at: number
}

const items = ref<TrashItem[]>([])
const total = ref(0)
const loading = ref(false)
const busy = ref<number | null>(null)

async function load() {
  loading.value = true
  try {
    const d = await get<{ items: TrashItem[]; total: number }>('/trash?limit=200')
    items.value = d.items
    total.value = d.total
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

function fmtDate(ts: number) {
  return new Date(ts * 1000).toLocaleString()
}

async function restore(item: TrashItem) {
  busy.value = item.image_id
  try {
    await post(`/trash/${item.image_id}/restore`, {})
    ElMessage.success('已恢复')
    await load()
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    busy.value = null
  }
}

async function purge(item: TrashItem) {
  await ElMessageBox.confirm('永久删除此图片？文件与记录将不可恢复。', '永久删除', {
    type: 'error',
    confirmButtonText: '永久删除',
  })
  busy.value = item.image_id
  try {
    await post(`/trash/${item.image_id}/purge`, {})
    ElMessage.success('已永久删除')
    await load()
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    busy.value = null
  }
}

async function purgeAll() {
  await ElMessageBox.confirm(`清空回收站（${total.value} 项）？所有文件与记录将不可恢复。`, '清空回收站', {
    type: 'error',
    confirmButtonText: '清空',
  })
  busy.value = -1
  try {
    const r = await post<{ purged: number }>('/trash/purge-all', {})
    ElMessage.success(`已清空 ${r.purged} 项`)
    await load()
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    busy.value = null
  }
}

onMounted(load)
</script>

<template>
  <div class="trash-page">
    <div class="toolbar">
      <span>回收站（{{ total }} 项）</span>
      <el-button
        type="danger"
        plain
        style="margin-left: auto"
        :disabled="total === 0"
        :loading="busy === -1"
        @click="purgeAll"
      >
        清空回收站
      </el-button>
    </div>

    <div v-loading="loading">
      <el-table :data="items" style="width: 100%">
        <el-table-column label="缩略图" width="100">
          <template #default="{ row }">
            <el-image
              class="thumb"
              :src="thumbUrl(row.thumb_rel)"
              fit="cover"
              :preview-src-list="[thumbUrl(row.thumb_rel)]"
            >
              <template #error>
                <div class="thumb-fallback">无图</div>
              </template>
            </el-image>
          </template>
        </el-table-column>
        <el-table-column label="原路径" prop="original_rel" />
        <el-table-column label="原因" width="130">
          <template #default="{ row }">
            <el-tag :type="row.reason === 'duplicate' ? 'warning' : 'info'" size="small">
              {{ row.reason === 'duplicate' ? '查重冗余' : row.reason }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="删除时间" width="180">
          <template #default="{ row }">
            <span class="num-mono">{{ fmtDate(row.deleted_at) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="200">
          <template #default="{ row }">
            <el-button
              size="small"
              type="primary"
              plain
              :loading="busy === row.image_id"
              @click="restore(row)"
            >
              恢复
            </el-button>
            <el-button
              size="small"
              type="danger"
              plain
              :loading="busy === row.image_id"
              @click="purge(row)"
            >
              永久删除
            </el-button>
          </template>
        </el-table-column>
      </el-table>
      <el-empty v-if="!loading && items.length === 0" description="回收站为空" />
    </div>
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
.thumb-fallback {
  width: 72px;
  height: 54px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--el-fill-color-light);
  color: var(--el-text-color-secondary);
  font-size: 11px;
}
</style>
