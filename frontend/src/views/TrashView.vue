<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { get, post } from '@/api/client'
import { reportLog } from '@/api/log'
import { thumbUrl } from '@/stores/dedup'
import { fmtBytes } from '@/utils/format'

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

/** 原因显示中文化。 */
function reasonLabel(reason: string): string {
  switch (reason) {
    case 'manual':
      return '手动删除'
    case 'duplicate':
      return '查重冗余'
    case 'test-cleanup':
      return '测试清理'
    default:
      return reason
  }
}

/** 原路径显示归一化（混合分隔符 → 统一反斜杠）。 */
function normPath(p: string): string {
  return p.replace(/\//g, '\\')
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
    reportLog(`永久删除回收站图片 #${item.image_id}（${item.rel_path}）`)
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
    reportLog(`清空回收站（${r.purged} 项）`)
    await load()
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    busy.value = null
  }
}

/* ---------- 一致性检查 / 修复 ---------- */

type IssueKind = 'half_recycled' | 'zombie_recycled' | 'lost_file'

interface ImageIssue {
  image_id: number
  rel_path: string
  kind: IssueKind
  file_in_recycle: boolean
  file_in_library: boolean
}

interface OrphanFile {
  rel_path: string
  bytes: number
}

interface RecycleAudit {
  issues: ImageIssue[]
  orphans: OrphanFile[]
  orphan_bytes: number
  bin_records: number
  recycled_images: number
  files_on_disk: number
}

interface RepairReport {
  healed_half: number
  removed_zombie: number
  removed_lost: number
  removed_orphans: number
  removed_empty_dirs: number
  freed_bytes: number
  errors: string[]
  dry_run: boolean
}

const audit = ref<RecycleAudit | null>(null)
const auditing = ref(false)
const repairing = ref(false)
const auditVisible = ref(false)

const issueLabel: Record<IssueKind, string> = {
  half_recycled: '半完成态（文件已在回收站，状态仍是正常）',
  zombie_recycled: '僵尸行（状态是回收站，文件与记录都已丢）',
  lost_file: '文件丢失（回收站有记录，文件已不在）',
}

const issueCount = computed(() => audit.value?.issues.length ?? 0)
const orphanCount = computed(() => audit.value?.orphans.length ?? 0)

async function runAudit() {
  auditing.value = true
  try {
    audit.value = await get<RecycleAudit>('/trash/audit')
    auditVisible.value = true
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    auditing.value = false
  }
}

async function runRepair(removeOrphans: boolean) {
  const a = audit.value
  const half = a?.issues.filter((i) => i.kind === 'half_recycled').length ?? 0
  const dead =
    a?.issues.filter((i) => i.kind === 'zombie_recycled' || i.kind === 'lost_file').length ?? 0
  const lines = [
    '半完成态补正：' + half + ' 张（补成正式回收，之后可在回收站里恢复）',
    '无效条目清理：' + dead + ' 张（文件已丢，留着只会永远恢复失败）',
    removeOrphans
      ? '孤儿文件删除：' + orphanCount.value + ' 个 / ' + fmtBytes(a?.orphan_bytes ?? 0)
      : '孤儿文件：保留不动',
  ]
  await ElMessageBox.confirm(lines.join('\n'), '确认修复回收站', {
    type: removeOrphans ? 'warning' : 'info',
    confirmButtonText: '执行修复',
  })
  repairing.value = true
  try {
    const r = await post<RepairReport>('/trash/repair', {
      dry_run: false,
      remove_orphans: removeOrphans,
    })
    const parts = [
      r.healed_half ? '补正 ' + r.healed_half + ' 张' : '',
      r.removed_zombie + r.removed_lost
        ? '清理无效条目 ' + (r.removed_zombie + r.removed_lost) + ' 张'
        : '',
      r.removed_orphans
        ? '删除孤儿文件 ' + r.removed_orphans + ' 个（' + fmtBytes(r.freed_bytes) + '）'
        : '',
      r.removed_empty_dirs ? '清理空目录 ' + r.removed_empty_dirs + ' 个' : '',
    ].filter(Boolean)
    if (parts.length) ElMessage.success('修复完成：' + parts.join('，'))
    else ElMessage.info('没有需要修复的内容')
    if (r.errors.length) {
      ElMessageBox.alert(r.errors.slice(0, 10).join('\n'), '部分项目修复失败', {
        type: 'warning',
      })
    }
    reportLog(
      '修复回收站：补正 ' +
        r.healed_half +
        '，清理 ' +
        (r.removed_zombie + r.removed_lost) +
        '，孤儿 ' +
        r.removed_orphans,
    )
    await load()
    await runAudit()
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    repairing.value = false
  }
}

onMounted(load)
</script>

<template>
  <div class="trash-page">
    <div class="toolbar">
      <span>回收站（{{ total }} 项）</span>
      <el-button
        size="small"
        plain
        :loading="auditing"
        style="margin-left: auto"
        @click="runAudit"
      >
        一致性检查
      </el-button>
      <el-button
        type="danger"
        plain
        :disabled="total === 0"
        :loading="busy === -1"
        @click="purgeAll"
      >
        清空回收站
      </el-button>
    </div>

    <!-- 一致性检查结果：回收站目录与数据库记录对不上时在这里暴露并可修复 -->
    <el-card v-if="auditVisible && audit" shadow="never" class="audit-card">
      <template #header>
        <div class="audit-head">
          <span>回收站一致性</span>
          <el-button link size="small" @click="auditVisible = false">收起</el-button>
        </div>
      </template>
      <div class="audit-summary">
        <span>记录 {{ audit.bin_records }} 条 / 状态为回收站 {{ audit.recycled_images }} 张 / 目录内文件
          {{ audit.files_on_disk }} 个</span>
        <el-tag v-if="issueCount === 0 && orphanCount === 0" type="success" size="small">
          一切正常
        </el-tag>
        <el-tag v-else type="warning" size="small">
          {{ issueCount }} 项数据问题 · {{ orphanCount }} 个孤儿文件（{{ fmtBytes(audit.orphan_bytes) }}）
        </el-tag>
      </div>

      <el-table v-if="audit.issues.length" :data="audit.issues" size="small" max-height="240">
        <el-table-column label="图片" width="80">
          <template #default="{ row }">#{{ row.image_id }}</template>
        </el-table-column>
        <el-table-column label="路径">
          <template #default="{ row }">
            <span class="num-mono">{{ normPath(row.rel_path) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="问题" width="320">
          <template #default="{ row }">
            <el-tag size="small" :type="row.kind === 'half_recycled' ? 'info' : 'danger'">
              {{ issueLabel[row.kind as IssueKind] }}
            </el-tag>
          </template>
        </el-table-column>
      </el-table>

      <div v-if="audit.orphans.length" class="orphan-list">
        <div class="orphan-title">孤儿文件（回收站目录里没有任何记录指向它们）</div>
        <div class="orphan-items">
          <span v-for="o in audit.orphans.slice(0, 30)" :key="o.rel_path" class="orphan-item num-mono">
            {{ normPath(o.rel_path) }}（{{ fmtBytes(o.bytes) }}）
          </span>
          <span v-if="audit.orphans.length > 30" class="orphan-item">
            ……共 {{ audit.orphans.length }} 个
          </span>
        </div>
        <div class="hint">
          其中 import_duplicate/ 下的是重复导入时被自动移走的副本，属预期产物，可安全删除。
        </div>
      </div>

      <div class="audit-actions">
        <el-button size="small" :loading="repairing" :disabled="issueCount === 0 && orphanCount === 0" @click="runRepair(false)">
          修复数据问题
        </el-button>
        <el-button
          size="small"
          type="danger"
          plain
          :loading="repairing"
          :disabled="orphanCount === 0"
          @click="runRepair(true)"
        >
          修复并清理孤儿文件
        </el-button>
      </div>
    </el-card>

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
        <el-table-column label="原路径">
          <template #default="{ row }">
            <span class="num-mono">{{ normPath(row.original_rel) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="原因" width="130">
          <template #default="{ row }">
            <el-tag :type="row.reason === 'duplicate' ? 'warning' : 'info'" size="small">
              {{ reasonLabel(row.reason) }}
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
.audit-card :deep(.el-card__header) {
  padding: 8px 12px;
}
.audit-card :deep(.el-card__body) {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.audit-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
  font-weight: 600;
}
.audit-summary {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.orphan-title {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 4px;
}
.orphan-items {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  max-height: 120px;
  overflow-y: auto;
}
.orphan-item {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--el-fill-color-light);
  color: var(--el-text-color-regular);
}
.hint {
  margin-top: 6px;
  font-size: 11px;
  color: var(--el-text-color-secondary);
}
.audit-actions {
  display: flex;
  gap: 8px;
}
</style>
