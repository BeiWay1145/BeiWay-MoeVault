<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import {
  DataAnalysis,
  Picture,
  Search,
  Connection,
  Delete,
  CollectionTag,
  Setting,
  FolderOpened,
} from '@element-plus/icons-vue'
import { useDedupStore } from '@/stores/dedup'

const route = useRoute()
const dedupStore = useDedupStore()

// 菜单高亮：图库详情页归入「图库」
const activeMenu = computed(() => {
  if (route.path.startsWith('/library')) return '/library'
  return route.path
})

const menus = [
  { path: '/', label: '总览', icon: DataAnalysis },
  { path: '/library', label: '图库', icon: Picture },
  { path: '/imports', label: '主目录', icon: FolderOpened },
  { path: '/search', label: '搜索', icon: Search },
  { path: '/dedup', label: '查重', icon: Connection, badge: 'dedup' },
  { path: '/trash', label: '回收站', icon: Delete },
  { path: '/tags', label: '标签', icon: CollectionTag },
]
</script>

<template>
  <el-aside width="200px" class="side-nav">
    <div class="logo">🖼 BeiWay-MoeVault</div>
    <el-menu :default-active="activeMenu" router class="side-menu">
      <el-menu-item v-for="m in menus" :key="m.path" :index="m.path">
        <el-icon><component :is="m.icon" /></el-icon>
        <span>{{ m.label }}</span>
        <el-badge
          v-if="m.badge === 'dedup' && dedupStore.redundantCount > 0"
          :value="dedupStore.redundantCount"
          class="menu-badge"
        />
      </el-menu-item>
      <el-menu-item index="/settings">
        <el-icon><Setting /></el-icon>
        <span>设置</span>
      </el-menu-item>
    </el-menu>
  </el-aside>
</template>

<style scoped>
.side-nav {
  background: var(--el-bg-color);
  border-right: 1px solid var(--el-border-color-light);
  display: flex;
  flex-direction: column;
  height: 100%;
}
.logo {
  padding: 18px 16px;
  font-size: 16px;
  font-weight: 600;
  border-bottom: 1px solid var(--el-border-color-lighter);
}
.side-menu {
  flex: 1;
  border-right: none;
  padding-top: 8px;
}
.menu-badge {
  margin-left: auto;
}
</style>
