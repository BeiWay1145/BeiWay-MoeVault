<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { useRoute } from 'vue-router'
import { ElMessage } from 'element-plus'
import SideNav from '@/components/SideNav.vue'
import TopBar from '@/components/TopBar.vue'
import DragImport from '@/components/DragImport.vue'
import { useTaskStore } from '@/stores/tasks'
import { startWsEvents, stopWsEvents } from '@/api/ws'

const taskStore = useTaskStore()
const route = useRoute()

// 全局启动任务轮询：任何页面提交任务后都能收到完成通知
onMounted(() => taskStore.start())
onUnmounted(() => taskStore.stop())

// 增强1：订阅后端 WS 广播——导入批次真正完成/失败时
// 1) 全局通知  2) 派发 'moevault:import-done' 供各视图刷新当前界面
//   （主目录 loadTree / 图库、搜索 fetchImages 各自监听）
interface ImportDonePayload {
  batch_id?: number
  done?: number
  failed?: number
  duplicate?: number
  error?: string
}
function onImportDone(e: Event) {
  const d = (e as CustomEvent).detail as ImportDonePayload
  ElMessage({
    message: `导入任务 #${d.batch_id ?? '?'} 已完成：成功 ${d.done ?? 0} 张${
      d.failed ? `，失败 ${d.failed} 张` : ''
    }${d.duplicate ? `，重复跳过 ${d.duplicate} 张` : ''}`,
    type: 'success',
    duration: 5000,
    showClose: true,
  })
}
function onImportFailed(e: Event) {
  const d = (e as CustomEvent).detail as ImportDonePayload
  ElMessage({
    message: `导入任务 #${d.batch_id ?? '?'} 失败：${d.error ?? '未知错误'}`,
    type: 'error',
    duration: 6000,
    showClose: true,
  })
}
onMounted(() => {
  startWsEvents()
  window.addEventListener('moevault:import-done', onImportDone)
  window.addEventListener('moevault:import-failed', onImportFailed)
})
onUnmounted(() => {
  stopWsEvents()
  window.removeEventListener('moevault:import-done', onImportDone)
  window.removeEventListener('moevault:import-failed', onImportFailed)
})
</script>

<template>
  <el-container class="app-layout">
    <SideNav />
    <el-container class="app-body">
      <TopBar />
      <el-main class="app-main">
        <!-- keepAlive 路由缓存：返回时保留筛选/展开/滚动状态 -->
        <router-view v-slot="{ Component }">
          <keep-alive :include="['library', 'imports']">
            <component :is="Component" :key="route.name" />
          </keep-alive>
        </router-view>
      </el-main>
    </el-container>
    <!-- 全局拖入导入（仅桌面壳生效；覆盖层 + 确认框挂全局） -->
    <DragImport />
  </el-container>
</template>

<style scoped>
.app-layout {
  height: 100%;
}
.app-body {
  min-width: 0;
  flex-direction: column;
}
.app-main {
  padding: 16px;
  overflow: auto;
  height: 100%;
  /* 侧边栏宽度动画期间：隔离主区绘制，减少表格重绘卡顿 */
  contain: paint;
}
</style>
