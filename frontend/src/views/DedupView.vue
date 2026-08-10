<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { useDedupStore } from '@/stores/dedup'

const dedup = useDedupStore()
interface MockGroup {
  id: number
  size: number
  redundant: number
  members: { id: number; hue: number; clarity: number; best: boolean }[]
}

const groups = ref<MockGroup[]>([])

onMounted(() => {
  dedup.loadMock()
  groups.value = Array.from({ length: 8 }, (_, i) => {
    const size = 2 + (i % 4)
    const members = Array.from({ length: size }, (_, j) => ({
      id: i * 10 + j,
      hue: (i * 47 + j * 61) % 360,
      clarity: +(2 + (size - j) + ((i * 7) % 10) / 10).toFixed(1),
      best: j === 0,
    }))
    return {
      id: 2000 + i,
      size,
      redundant: size - 1,
      members,
    }
  })
})

function resolveGroup(g: MockGroup) {
  ElMessage.success(`组 #${g.id}: 保留最优，${g.redundant} 张冗余候选将入回收站（骨架占位）`)
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
      <el-button type="primary" plain style="margin-left: auto">全库重扫（骨架占位）</el-button>
    </div>

    <el-card v-for="g in groups" :key="g.id" class="group-card">
      <template #header>
        <div class="group-head">
          <span>组 #{{ g.id }} · 共 {{ g.size }} 张 · 冗余 {{ g.redundant }} 张</span>
          <span>
            <el-button size="small" @click="ElMessage.info('对比视图（骨架占位）')">对比</el-button>
            <el-button size="small" type="primary" @click="resolveGroup(g)">保留最优 → 回收站</el-button>
          </span>
        </div>
      </template>
      <div class="members">
        <div v-for="m in g.members" :key="m.id" class="member" :class="{ best: m.best }">
          <div
            class="member-thumb"
            :style="`background: hsl(${m.hue} 65% 72%)`"
            :title="m.best ? '最优（清晰度最高）' : '冗余候选'"
          >
            <span v-if="m.best" class="mk best-mk">✓</span>
            <span v-else class="mk warn-mk">⚠</span>
          </div>
          <div class="member-clarity num-mono">清晰度 {{ m.clarity.toFixed(1) }}</div>
        </div>
      </div>
    </el-card>

    <el-empty v-if="groups.length === 0" description="暂无重复组" />
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
.members {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}
.member {
  width: 120px;
}
.member-thumb {
  height: 84px;
  border-radius: 6px;
  position: relative;
  border: 2px solid transparent;
}
.member.best .member-thumb {
  border-color: var(--el-color-success);
}
.mk {
  position: absolute;
  top: 4px;
  right: 4px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  color: #fff;
  font-size: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.best-mk {
  background: var(--el-color-success);
}
.warn-mk {
  background: var(--el-color-warning);
}
.member-clarity {
  font-size: 11px;
  color: var(--el-text-color-secondary);
  margin-top: 4px;
  text-align: center;
}
</style>
