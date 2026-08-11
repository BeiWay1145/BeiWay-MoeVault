<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Plus, Search } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { get, put, post, del } from '@/api/client'

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
const keyword = ref('')
// 分页
const page = ref(1)
const pageSize = ref(50)
const total = ref(0)
// 批量选择
const selected = ref<Set<number>>(new Set())

const categoryOptions = [
  { value: 'artist', label: '画师' },
  { value: 'copyright', label: '系列/作品' },
  { value: 'character', label: '角色' },
  { value: 'general', label: '常规' },
]

async function loadTags() {
  loading.value = true
  try {
    const offset = (page.value - 1) * pageSize.value
    const d = await get<{ items: TagItem[]; total: number }>(
      `/tags?offset=${offset}&limit=${pageSize.value}${keyword.value.trim() ? `&q=${encodeURIComponent(keyword.value.trim())}` : ''}`,
    )
    tags.value = d.items
    total.value = d.total
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

/** 搜索（防抖 + 回第一页）。 */
let searchTimer: number | undefined
function onSearchInput() {
  if (searchTimer !== undefined) window.clearTimeout(searchTimer)
  searchTimer = window.setTimeout(() => {
    page.value = 1
    loadTags()
  }, 300)
}

async function changeCategory(t: TagItem, category: string) {
  if (category === t.category) return
  try {
    await put(`/tags/${t.id}/category`, { category })
    t.category = category
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 删除单个标签。 */
async function removeTag(t: TagItem) {
  try {
    await ElMessageBox.confirm(`删除标签「${t.name}」？其图片关联也会删除。`, '删除标签', { type: 'warning' })
  } catch {
    return
  }
  try {
    await del(`/tags/${t.id}`)
    ElMessage.success('已删除')
    await loadTags()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 拉黑/取消拉黑单个标签。 */
async function toggleBlacklist(t: TagItem) {
  try {
    await post(`/tags/${t.id}/blacklist`, { blacklisted: !t.is_blacklisted })
    t.is_blacklisted = !t.is_blacklisted
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 批量删除。 */
async function batchDelete() {
  const ids = [...selected.value]
  if (ids.length === 0) return
  try {
    await ElMessageBox.confirm(`批量删除 ${ids.length} 个标签？图片关联也会删除。`, '批量删除', { type: 'warning' })
  } catch {
    return
  }
  try {
    await post('/tags/batch-delete', { ids })
    ElMessage.success(`已删除 ${ids.length} 个标签`)
    selected.value = new Set()
    await loadTags()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 批量拉黑。 */
async function batchBlacklist() {
  const ids = [...selected.value]
  if (ids.length === 0) return
  try {
    await post('/tags/batch-blacklist', { ids, blacklisted: true })
    ElMessage.success(`已拉黑 ${ids.length} 个标签（不再显示）`)
    selected.value = new Set()
    await loadTags()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

function toggleSelect(id: number) {
  const s = new Set(selected.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  selected.value = s
}

function createTag() {
  if (!newName.value.trim()) return
  ElMessage.info('自定义标签创建将在标签管理 API 完善后生效')
  newName.value = ''
}

onMounted(loadTags)
</script>

<template>
  <div class="tags-page">
    <div class="toolbar">
      <el-input v-model="newName" placeholder="新建自定义标签名" style="width: 200px" @keyup.enter="createTag" />
      <el-button type="primary" :icon="Plus" @click="createTag">新建</el-button>
      <el-input
        v-model="keyword"
        placeholder="搜索标签（名称/中文）"
        style="width: 220px"
        :prefix-icon="Search"
        clearable
        @input="onSearchInput"
        @clear="loadTags"
      />
      <template v-if="selected.size > 0">
        <el-button type="danger" plain @click="batchDelete">批量删除 ({{ selected.size }})</el-button>
        <el-button plain @click="batchBlacklist">批量拉黑 ({{ selected.size }})</el-button>
      </template>
      <div class="spacer" />
      <el-switch v-model="cnDict" active-text="中文字典" inactive-text="英文" />
    </div>

    <div v-loading="loading">
      <el-table v-if="tags.length > 0" :data="tags" @selection-change="(rows: TagItem[]) => { selected = new Set(rows.map(r => r.id)) }">
        <el-table-column type="selection" width="45" />
        <el-table-column prop="name" label="名称(EN)" />
        <el-table-column prop="name_cn" label="中文" width="120" />
        <el-table-column label="分类" width="140">
          <template #default="{ row }">
            <el-select
              :model-value="row.category"
              size="small"
              style="width: 110px"
              @change="(c: string) => changeCategory(row, c)"
            >
              <el-option v-for="o in categoryOptions" :key="o.value" :label="o.label" :value="o.value" />
            </el-select>
          </template>
        </el-table-column>
        <el-table-column prop="image_count" label="关联图数" width="90">
          <template #default="{ row }">
            <span class="num-mono">{{ row.image_count }}</span>
          </template>
        </el-table-column>
        <el-table-column label="黑名单" width="90">
          <template #default="{ row }">
            <el-tag :type="row.is_blacklisted ? 'danger' : 'info'" size="small">
              {{ row.is_blacklisted ? '已拉黑' : '正常' }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="150">
          <template #default="{ row }">
            <el-button size="small" :type="row.is_blacklisted ? 'primary' : 'warning'" plain @click="toggleBlacklist(row)">
              {{ row.is_blacklisted ? '取消拉黑' : '拉黑' }}
            </el-button>
            <el-button size="small" type="danger" plain @click="removeTag(row)">删除</el-button>
          </template>
        </el-table-column>
      </el-table>
      <el-empty v-else-if="!loading" description="暂无标签，导入图片并打标后自动出现" />
    </div>

    <div class="pager">
      <el-pagination
        layout="total, prev, pager, next"
        :total="total"
        :page-size="pageSize"
        :current-page="page"
        @current-change="(p: number) => { page = p; loadTags() }"
      />
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
.pager {
  display: flex;
  justify-content: center;
}
</style>
