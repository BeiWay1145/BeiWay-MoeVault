import { createRouter, createWebHistory } from 'vue-router'
import AppLayout from '@/layouts/AppLayout.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      component: AppLayout,
      children: [
        { path: '', name: 'dashboard', component: () => import('@/views/DashboardView.vue'), meta: { title: '总览' } },
        { path: 'library', name: 'library', component: () => import('@/views/LibraryView.vue'), meta: { title: '图库' } },
        { path: 'library/:id', name: 'image-detail', component: () => import('@/views/ImageDetailView.vue'), meta: { title: '图片详情' } },
        { path: 'search', name: 'search', component: () => import('@/views/SearchView.vue'), meta: { title: '搜索' } },
        { path: 'dedup', name: 'dedup', component: () => import('@/views/DedupView.vue'), meta: { title: '查重' } },
        { path: 'trash', name: 'trash', component: () => import('@/views/TrashView.vue'), meta: { title: '回收站' } },
        { path: 'tasks', name: 'tasks', component: () => import('@/views/TaskCenterView.vue'), meta: { title: '任务中心' } },
        { path: 'tags', name: 'tags', component: () => import('@/views/TagManageView.vue'), meta: { title: '标签管理' } },
        { path: 'settings', name: 'settings', component: () => import('@/views/SettingsView.vue'), meta: { title: '设置' } },
      ],
    },
  ],
})

router.afterEach((to) => {
  document.title = to.meta.title ? `${to.meta.title as string} · BeiWay-MoeVault` : 'BeiWay-MoeVault'
})

export default router
