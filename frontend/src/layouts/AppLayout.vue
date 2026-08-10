<script setup lang="ts">
import SideNav from '@/components/SideNav.vue'
import TopBar from '@/components/TopBar.vue'
</script>

<template>
  <el-container class="app-layout">
    <SideNav />
    <el-container class="app-body">
      <TopBar />
      <el-main class="app-main">
        <router-view />
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
