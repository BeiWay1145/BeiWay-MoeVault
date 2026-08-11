<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { Sunny, Moon, List } from '@element-plus/icons-vue'

const router = useRouter()

// 主题切换（暗黑/亮色，持久化 localStorage）
const isDark = ref(document.documentElement.classList.contains('dark'))
function toggleTheme() {
  isDark.value = !isDark.value
  document.documentElement.classList.toggle('dark', isDark.value)
  localStorage.setItem('moevault-theme', isDark.value ? 'dark' : 'light')
}
</script>

<template>
  <el-header class="top-bar">
    <div class="left">
      <span class="app-title">BeiWay-MoeVault</span>
    </div>
    <div class="right">
      <el-button :icon="List" text @click="router.push('/tasks')" title="任务中心" />
      <el-button :icon="isDark ? Sunny : Moon" text @click="toggleTheme" :title="isDark ? '切换亮色模式' : '切换暗黑模式'" />
    </div>
  </el-header>
</template>

<style scoped>
.top-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color-light);
  height: 56px;
}
.left,
.right {
  display: flex;
  align-items: center;
  gap: 4px;
}
.app-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--el-text-color-primary);
}
</style>
