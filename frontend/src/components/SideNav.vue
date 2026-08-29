<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import {
  DataAnalysis,
  Picture,
  Connection,
  Delete,
  CollectionTag,
  Setting,
  FolderOpened,
  Expand,
  Fold,
} from '@element-plus/icons-vue'
import { useDedupStore } from '@/stores/dedup'
import { useSettingsStore } from '@/stores/settings'

const route = useRoute()
const dedupStore = useDedupStore()
const settingsStore = useSettingsStore()

// 侧边栏收起状态（localStorage 持久化）
const SIDEBAR_KEY = 'moevault-sidebar-collapsed'
const collapsed = ref(false)

// 悬停自动展开（通用设置可配，默认开）——响应式：设置变化即时生效
const hoverExpand = ref(true)
let hoverTimer: number | undefined
let leaveTimer: number | undefined

onMounted(async () => {
  try {
    collapsed.value = localStorage.getItem(SIDEBAR_KEY) === '1'
  } catch {
    /* 忽略 */
  }
  await settingsStore.load().catch(() => {})
  hoverExpand.value = settingsStore.settings.sidebar_hover_expand
})

// 设置页修改悬停展开 → 即时生效（无需重启前端）
watch(
  () => settingsStore.settings.sidebar_hover_expand,
  (v) => {
    hoverExpand.value = v
    // 关闭悬停展开时清理待执行的收起定时器，避免误收起
    if (!v && leaveTimer !== undefined) {
      window.clearTimeout(leaveTimer)
      leaveTimer = undefined
    }
  },
)

function toggleCollapse() {
  collapsed.value = !collapsed.value
  try {
    localStorage.setItem(SIDEBAR_KEY, collapsed.value ? '1' : '0')
  } catch {
    /* 忽略 */
  }
}

function onMouseEnter() {
  if (!collapsed.value || !hoverExpand.value) return
  if (leaveTimer !== undefined) window.clearTimeout(leaveTimer)
  hoverTimer = window.setTimeout(() => {
    collapsed.value = false
  }, 250)
}
function onMouseLeave() {
  if (hoverTimer !== undefined) window.clearTimeout(hoverTimer)
  // 仅悬停自动展开开启时，离开才收回；关闭时侧边栏保持手动展开状态（只有按钮能收起）
  if (hoverExpand.value) {
    leaveTimer = window.setTimeout(() => {
      collapsed.value = true
    }, 300)
  } else if (leaveTimer !== undefined) {
    window.clearTimeout(leaveTimer)
  }
}
onUnmounted(() => {
  if (hoverTimer !== undefined) window.clearTimeout(hoverTimer)
  if (leaveTimer !== undefined) window.clearTimeout(leaveTimer)
})

// 菜单高亮：图库详情页归入「图库」
const activeMenu = computed(() => {
  if (route.path.startsWith('/library')) return '/library'
  return route.path
})

const menus = [
  { path: '/', label: '总览', icon: DataAnalysis },
  { path: '/library', label: '图库', icon: Picture },
  { path: '/imports', label: '主目录', icon: FolderOpened },
  { path: '/dedup', label: '查重结果', icon: Connection, badge: 'dedup' },
  { path: '/trash', label: '回收站', icon: Delete },
  { path: '/tags', label: '标签', icon: CollectionTag },
]
</script>

<template>
  <el-aside
    :width="collapsed ? '64px' : '200px'"
    class="side-nav"
    :class="{ collapsed }"
    @mouseenter="onMouseEnter"
    @mouseleave="onMouseLeave"
  >
    <div class="logo">
      <span class="logo-icon">🖼</span>
      <span v-if="!collapsed" class="logo-text">BeiWay-MoeVault</span>
    </div>
    <el-menu :default-active="activeMenu" router class="side-menu" :collapse="collapsed" :collapse-transition="false">
      <el-menu-item v-for="m in menus" :key="m.path" :index="m.path">
        <el-icon><component :is="m.icon" /></el-icon>
        <template #title>
          <span>{{ m.label }}</span>
          <el-badge
            v-if="m.badge === 'dedup' && dedupStore.redundantCount > 0"
            :value="dedupStore.redundantCount"
            class="menu-badge"
          />
        </template>
      </el-menu-item>
      <el-menu-item index="/settings">
        <el-icon><Setting /></el-icon>
        <template #title><span>设置</span></template>
      </el-menu-item>
    </el-menu>
    <button class="collapse-btn" :title="collapsed ? '展开侧边栏' : '收起侧边栏'" @click="toggleCollapse">
      <el-icon><Expand v-if="collapsed" /><Fold v-else /></el-icon>
    </button>
  </el-aside>
</template>

<style scoped>
.side-nav {
  background: var(--el-bg-color);
  border-right: 1px solid var(--el-border-color-light);
  display: flex;
  flex-direction: column;
  height: 100%;
  /* 侧边栏展开/收起动画（保留）；卡顿缓解见 .app-main contain + 表格 fixed 布局 */
  transition: width 0.22s ease;
  overflow: hidden;
}
.logo {
  padding: 18px 16px;
  font-size: 16px;
  font-weight: 600;
  border-bottom: 1px solid var(--el-border-color-lighter);
  display: flex;
  align-items: center;
  gap: 8px;
  white-space: nowrap;
}
.side-nav.collapsed .logo {
  justify-content: center;
  padding: 18px 0;
}
.side-nav.collapsed .logo-text {
  display: none;
}
.side-menu {
  flex: 1;
  border-right: none;
  padding-top: 8px;
}
.side-nav.collapsed .side-menu {
  --el-menu-icon-width: 24px;
}
.menu-badge {
  margin-left: 8px;
}
.collapse-btn {
  border: none;
  background: transparent;
  cursor: pointer;
  padding: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-secondary);
  border-top: 1px solid var(--el-border-color-lighter);
  font-size: 16px;
  transition: color 0.15s;
}
.collapse-btn:hover {
  color: var(--el-color-primary);
  background: var(--el-fill-color-light);
}
</style>
