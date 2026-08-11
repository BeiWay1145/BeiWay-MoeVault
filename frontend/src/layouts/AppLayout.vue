<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { useRoute } from 'vue-router'
import SideNav from '@/components/SideNav.vue'
import TopBar from '@/components/TopBar.vue'
import { useTaskStore } from '@/stores/tasks'

const taskStore = useTaskStore()
const route = useRoute()

// 全局启动任务轮询：任何页面提交任务后都能收到完成通知
onMounted(() => taskStore.start())
onUnmounted(() => taskStore.stop())
</script>

<template>
  <el-container class="app-layout">
    <SideNav />
    <el-container class="app-body">
      <TopBar />
      <el-main class="app-main">
        <!-- keepAlive 路由缓存：返回时保留筛选/展开/滚动状态 -->
        <router-view v-slot="{ Component }">
          <keep-alive :include="['library', 'imports', 'search']">
            <component :is="Component" :key="route.name" />
          </keep-alive>
        </router-view>
      </el-main>
    </el-container>
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
}
</style>
