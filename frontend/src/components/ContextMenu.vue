<script setup lang="ts">
/**
 * 通用右键菜单（图库 / 主目录共用）。
 *
 * 行为（按需求）：
 * - 右键**未选中**的图片 → 菜单只作用于该图（不使用现有选择集）
 * - 右键**已选中**的图片 → 菜单作用于**整个选择集**（单选或多选均可）
 * - **无需二次确认**：菜单里的批量行为直接执行
 *
 * 菜单项来自批量操作注册表（constants/batchActions.ts），
 * 因此工具栏与右键菜单的能力永远一致，不会出现两套逻辑。
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { actionsFor, type BatchAction, type BatchActionContext } from '@/constants/batchActions'

const props = defineProps<{
  /** 执行上下文里的参数对象（由父级持有，菜单只读写）。 */
  options: BatchActionContext['options']
  /** 刷新回调。 */
  refresh?: () => void
  /** 打开导出弹窗（图库注入）。 */
  openExport?: (ids: number[]) => void
  /** 清理选择。 */
  clearSelection?: () => void
  /** 把图片 id 解析成磁盘路径（拖出/复制类动作需要）。 */
  resolvePaths?: (ids: number[]) => string[]
}>()

const visible = ref(false)
const x = ref(0)
const y = ref(0)
/** 本次菜单作用的图片 id（外部通过 open() 传入）。 */
const targetIds = ref<number[]>([])

const actions = computed(() => actionsFor('context'))

/** 打开菜单。ids 由调用方根据"右键的图是否已选中"决定。 */
function open(event: MouseEvent, ids: number[]) {
  event.preventDefault()
  targetIds.value = ids
  visible.value = true
  // 先渲染再测量，保证不超出视口
  requestAnimationFrame(() => {
    const el = menuRef.value
    const w = el?.offsetWidth ?? 180
    const h = el?.offsetHeight ?? 240
    x.value = Math.min(event.clientX, window.innerWidth - w - 8)
    y.value = Math.min(event.clientY, window.innerHeight - h - 8)
  })
  x.value = event.clientX
  y.value = event.clientY
}

function close() {
  visible.value = false
}

const menuRef = ref<HTMLElement | null>(null)
const running = ref<string | null>(null)

async function run(action: BatchAction) {
  const ids = [...targetIds.value]
  if (ids.length === 0 || running.value) return
  running.value = action.key
  try {
    await action.run({
      ids,
      options: props.options,
      refresh: props.refresh,
      openExport: props.openExport,
      clearSelection: props.clearSelection,
      resolvePaths: props.resolvePaths,
    })
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    running.value = null
    close()
  }
}

// 点击空白/滚动/ESC 关闭
function onDocClick() {
  if (visible.value) close()
}
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && visible.value) close()
}
onMounted(() => {
  document.addEventListener('click', onDocClick)
  document.addEventListener('contextmenu', onDocClick, true)
  window.addEventListener('scroll', onDocClick, true)
  window.addEventListener('keydown', onKeydown)
})
onBeforeUnmount(() => {
  document.removeEventListener('click', onDocClick)
  document.removeEventListener('contextmenu', onDocClick, true)
  window.removeEventListener('scroll', onDocClick, true)
  window.removeEventListener('keydown', onKeydown)
})

defineExpose({ open, close })
</script>

<template>
  <Teleport to="body">
    <div
      v-if="visible"
      ref="menuRef"
      class="ctx-menu"
      :style="{ left: x + 'px', top: y + 'px' }"
      @click.stop
      @contextmenu.prevent
    >
      <div class="ctx-count">
        作用于 {{ targetIds.length }} 张图片
      </div>
      <div
        v-for="a in actions"
        :key="a.key"
        class="ctx-item"
        :class="{ danger: a.danger, disabled: running !== null && running !== a.key }"
        @click="run(a)"
      >
        <el-icon v-if="a.icon" class="ctx-icon"><component :is="a.icon" /></el-icon>
        <span class="ctx-label">{{ a.label }}</span>
        <span v-if="running === a.key" class="ctx-running">执行中…</span>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.ctx-menu {
  position: fixed;
  z-index: 4000;
  min-width: 170px;
  padding: 4px;
  background: var(--el-bg-color-overlay);
  border: 1px solid var(--el-border-color-light);
  border-radius: 8px;
  box-shadow: var(--el-box-shadow);
  font-size: 13px;
  user-select: none;
}
.ctx-count {
  padding: 4px 10px 6px;
  font-size: 11px;
  color: var(--el-text-color-secondary);
  border-bottom: 1px solid var(--el-border-color-lighter);
  margin-bottom: 4px;
}
.ctx-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-radius: 5px;
  cursor: pointer;
  color: var(--el-text-color-regular);
}
.ctx-item:hover {
  background: var(--el-color-primary-light-9);
  color: var(--el-color-primary);
}
.ctx-item.danger {
  color: var(--el-color-danger);
}
.ctx-item.danger:hover {
  background: var(--el-color-danger-light-9);
}
.ctx-item.disabled {
  opacity: 0.45;
  pointer-events: none;
}
.ctx-icon {
  font-size: 14px;
}
.ctx-label {
  flex: 1;
}
.ctx-running {
  font-size: 11px;
  color: var(--el-text-color-secondary);
}
</style>
