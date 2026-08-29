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
const newName = ref('')
const keyword = ref('')
// 分页（增强4：每页数独立 localStorage，与图库无关）
const page = ref(1)
const pageSize = ref(Number(localStorage.getItem('moevault-tags-page-size') || '100'))
const total = ref(0)
// 增强5：筛选（分类 + 未设中文别名）
const filterCategory = ref('')
const filterNoCn = ref(false)
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
    const params = new URLSearchParams()
    params.set('offset', String(offset))
    params.set('limit', String(pageSize.value))
    if (keyword.value.trim()) params.set('q', keyword.value.trim())
    if (filterCategory.value) params.set('category', filterCategory.value)
    if (filterNoCn.value) params.set('no_cn', '1')
    const d = await get<{ items: TagItem[]; total: number }>(`/tags?${params.toString()}`)
    tags.value = d.items
    total.value = d.total
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

/** 增强4：每页条数修改立即生效并持久化（BUG2：接收 size 参数更新 ref）。 */
function onPageSizeChange(s: number) {
  pageSize.value = s
  localStorage.setItem('moevault-tags-page-size', String(s))
  page.value = 1
  loadTags()
}

/** 增强5：筛选变化回到第一页刷新。 */
function onFilterChange() {
  page.value = 1
  loadTags()
}

// ---- 增强3：中文别名管理（一个 tag 多条别名）----
interface TagAlias {
  id: number
  alias: string
}
const aliasDialogVisible = ref(false)
const aliasTarget = ref<TagItem | null>(null)
const aliasList = ref<TagAlias[]>([])
const newAlias = ref('')

async function openAliasDialog(t: TagItem) {
  aliasTarget.value = t
  newAlias.value = ''
  aliasDialogVisible.value = true
  await loadAliases()
}

async function loadAliases() {
  if (!aliasTarget.value) return
  try {
    const d = await get<{ aliases: TagAlias[] }>(`/tags/${aliasTarget.value.id}/aliases`)
    aliasList.value = d.aliases
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function addAlias() {
  if (!aliasTarget.value || !newAlias.value.trim()) return
  try {
    await post(`/tags/${aliasTarget.value.id}/aliases`, { alias: newAlias.value.trim() })
    newAlias.value = ''
    await loadAliases()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function removeAlias(a: TagAlias) {
  if (!aliasTarget.value) return
  try {
    await del(`/tags/${aliasTarget.value.id}/aliases/${a.id}`)
    await loadAliases()
  } catch (e) {
    ElMessage.error((e as Error).message)
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

/** 设置/清除中文别名（增强2：别名参与搜索联想；留空=清除）。 */
async function editNameCn(t: TagItem) {
  const { value } = await ElMessageBox.prompt(
    '输入中文别名（用于搜索联想与显示；留空清除）',
    `中文别名 · ${t.name}`,
    {
      inputValue: t.name_cn ?? '',
      inputPlaceholder: '如：女孩 / 黑发',
    },
  ).catch(() => ({ value: null as string | null }))
  if (value === null) return
  try {
    await put(`/tags/${t.id}/name-cn`, { name_cn: value || null })
    t.name_cn = value.trim() || null
    ElMessage.success(value.trim() ? `已设置中文别名「${value.trim()}」` : '已清除中文别名')
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
        style="width: 200px"
        :prefix-icon="Search"
        clearable
        @input="onSearchInput"
        @clear="onFilterChange"
      />
      <!-- 增强5：筛选（分类 + 未设中文别名） -->
      <el-select v-model="filterCategory" style="width: 130px" @change="onFilterChange">
        <el-option label="全部分类" value="" />
        <el-option v-for="o in categoryOptions" :key="o.value" :label="o.label" :value="o.value" />
      </el-select>
      <el-checkbox v-model="filterNoCn" @change="onFilterChange">未设中文别名</el-checkbox>
      <template v-if="selected.size > 0">
        <el-button type="danger" plain @click="batchDelete">批量删除 ({{ selected.size }})</el-button>
        <el-button plain @click="batchBlacklist">批量拉黑 ({{ selected.size }})</el-button>
      </template>
      <div class="spacer" />
    </div>

    <div v-loading="loading">
      <el-table
        v-if="tags.length > 0"
        :data="tags"
        class="tag-table-fixed"
        @selection-change="(rows: TagItem[]) => { selected = new Set(rows.map(r => r.id)) }"
      >
        <el-table-column type="selection" width="45" />
        <el-table-column prop="name" label="名称(EN)" />
        <el-table-column label="中文别名" width="220">
          <template #default="{ row }">
            <span v-if="row.name_cn" class="num-mono">{{ row.name_cn }}</span>
            <span v-else class="muted">未设置</span>
            <el-button size="small" text type="primary" @click="editNameCn(row)">主别名</el-button>
            <el-button size="small" text type="success" @click="openAliasDialog(row)">多别名</el-button>
          </template>
        </el-table-column>
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
        layout="total, sizes, prev, pager, next"
        :total="total"
        :page-size="pageSize"
        :current-page="page"
        :page-sizes="[50, 100, 200, 500]"
        @current-change="(p: number) => { page = p; loadTags() }"
        @size-change="onPageSizeChange"
      />
    </div>

    <!-- 增强3：多中文别名管理弹窗 -->
    <el-dialog v-model="aliasDialogVisible" :title="`中文别名 · ${aliasTarget?.name ?? ''}`" width="460px" append-to-body>
      <div class="alias-list">
        <div v-for="a in aliasList" :key="a.id" class="alias-row">
          <span class="alias-text">{{ a.alias }}</span>
          <el-button size="small" text type="danger" @click="removeAlias(a)">删除</el-button>
        </div>
        <el-empty v-if="aliasList.length === 0" description="暂无别名" :image-size="40" />
      </div>
      <div class="alias-add">
        <el-input v-model="newAlias" placeholder="输入新别名，如：黑色头发" @keyup.enter="addAlias" />
        <el-button type="primary" @click="addAlias">添加</el-button>
      </div>
    </el-dialog>
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
.alias-list {
  max-height: 260px;
  overflow: auto;
  margin-bottom: 10px;
}
.alias-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 8px;
  border-bottom: 1px solid var(--el-border-color-lighter);
}
.alias-text {
  font-size: 13px;
}
.alias-add {
  display: flex;
  gap: 8px;
}
/* 侧边栏动画期间减少重排：表格 fixed 布局（列宽由表头决定，行内容不触发重算） */
.tag-table-fixed :deep(table) {
  table-layout: fixed;
  width: 100%;
}
</style>
