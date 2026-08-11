<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useDedupStore, thumbUrl, type DedupGroup, type GroupMember } from '@/stores/dedup'

const dedup = useDedupStore()
const expanded = ref<Set<number>>(new Set())
const membersCache = ref<Record<number, GroupMember[]>>({})
const scanning = ref(false)
const resolving = ref<number | null>(null)

onMounted(() => {
  dedup.fetchGroups().catch((e: Error) => ElMessage.error(e.message))
})

async function toggleExpand(g: DedupGroup) {
  if (expanded.value.has(g.id)) {
    expanded.value.delete(g.id)
    return
  }
  expanded.value.add(g.id)
  if (!membersCache.value[g.id]) {
    try {
      const detail = await dedup.groupDetail(g.id)
      membersCache.value[g.id] = detail.members
    } catch (e) {
      ElMessage.error((e as Error).message)
    }
  }
}

async function runScan() {
  scanning.value = true
  try {
    await dedup.scan(true)
    ElMessage.success('全库查重任务已启动（完成后 WS 自动刷新）')
    setTimeout(() => dedup.fetchGroups(), 2000)
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    scanning.value = false
  }
}

async function resolveBest(g: DedupGroup) {
  await ElMessageBox.confirm(
    `组 #${g.id} 共 ${g.size} 张，将保留最优（最清晰），其余 ${g.redundant_count} 张移入回收站（可恢复）。`,
    '确认查重结果',
    { type: 'warning', confirmButtonText: '保留最优，其余入回收站' },
  )
  resolving.value = g.id
  try {
    const r = await dedup.resolve(g.id, 'best_only')
    ElMessage.success(`已回收 ${r.recycled} 张冗余图`)
    membersCache.value[g.id] = []
    expanded.value.delete(g.id)
    await dedup.fetchGroups()
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    resolving.value = null
  }
}
</script>

<template>
  <div class="dedup-page">
    <div class="stat-bar">
      <el-statistic title="重复组" :value="dedup.groupCount" />
      <el-statistic title="涉及图片" :value="dedup.involvedImages" />
      <el-statistic title="冗余候选" :value="dedup.redundantCount">
        <template #suffix><span style="font-size: 12px">张</span></template>
      </el-statistic>
      <span class="hint">从主目录发起范围查重后，结果在这里处理</span>
    </div>

    <div v-loading="dedup.loading" class="group-list">
      <el-empty v-if="!dedup.loading && dedup.groups.length === 0" description="暂无重复组" />

      <el-card v-for="g in dedup.groups" :key="g.id" class="group-card">
        <template #header>
          <div class="group-head">
            <span class="group-title">
              组 #{{ g.id }}
              <el-tag v-if="g.redundant_count > 0" type="warning" size="small">
                冗余 {{ g.redundant_count }}
              </el-tag>
              <el-tag v-else type="success" size="small">无冗余</el-tag>
              <span class="group-size num-mono">共 {{ g.size }} 张</span>
            </span>
            <span class="group-actions">
              <el-button size="small" @click="toggleExpand(g)">
                {{ expanded.has(g.id) ? '收起' : '展开' }}
              </el-button>
              <el-button
                v-if="g.redundant_count > 0"
                size="small"
                type="primary"
                :loading="resolving === g.id"
                @click="resolveBest(g)"
              >
                保留最优 → 回收站
              </el-button>
            </span>
          </div>
        </template>

        <div v-if="expanded.has(g.id)" class="members">
          <div
            v-for="m in membersCache[g.id] ?? []"
            :key="m.image_id"
            class="member"
            :class="{ best: m.is_best }"
          >
            <el-image
              class="member-thumb"
              :src="thumbUrl(m.thumb_rel)"
              fit="cover"
              :preview-src-list="[thumbUrl(m.thumb_rel)]"
            >
              <template #error>
                <div class="thumb-fallback">无图</div>
              </template>
            </el-image>
            <div class="member-tags">
              <el-tag v-if="m.is_best" type="success" size="small">✓ 最优</el-tag>
              <el-tag v-if="m.is_redundant" type="warning" size="small">⚠ 冗余</el-tag>
            </div>
            <div class="member-info num-mono">
              清晰度 {{ m.clarity_score.toFixed(2) }}<br />
              {{ m.width }}×{{ m.height }}
            </div>
          </div>
        </div>
        <div v-else class="best-preview">
          <el-image
            v-if="g.best_thumb_rel"
            class="best-thumb"
            :src="thumbUrl(g.best_thumb_rel)"
            fit="cover"
          />
          <span class="best-text num-mono">
            最优清晰度：{{ g.best_clarity?.toFixed(2) ?? '—' }}
          </span>
        </div>
      </el-card>
    </div>
  </div>
</template>

<style scoped>
.dedup-page {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.stat-bar {
  display: flex;
  gap: 40px;
  align-items: center;
  padding: 12px 16px;
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 8px;
}
.group-card {
  margin-bottom: 4px;
}
.group-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.group-title {
  display: flex;
  align-items: center;
  gap: 8px;
}
.group-size {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
.members {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}
.member {
  width: 132px;
  border-radius: 8px;
  overflow: hidden;
  border: 2px solid var(--el-border-color-lighter);
  background: var(--el-bg-color);
}
.member.best {
  border-color: var(--el-color-success);
}
.member-thumb {
  width: 128px;
  height: 90px;
}
.thumb-fallback {
  width: 128px;
  height: 90px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color-light);
  font-size: 12px;
}
.member-tags {
  padding: 4px 6px 0;
  display: flex;
  gap: 4px;
}
.member-info {
  padding: 4px 6px 6px;
  font-size: 11px;
  color: var(--el-text-color-secondary);
}
.best-preview {
  display: flex;
  align-items: center;
  gap: 12px;
}
.best-thumb {
  width: 96px;
  height: 68px;
  border-radius: 6px;
}
.best-text {
  color: var(--el-text-color-secondary);
  font-size: 13px;
}
</style>
