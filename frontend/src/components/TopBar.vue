<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { Plus, Sunny, List } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'

const router = useRouter()
const importVisible = ref(false)
const importPaths = ref('')
const importMode = ref<'move' | 'copy'>('move')
const submitting = ref(false)

function onImportConfirm() {
  // 骨架阶段：仅前端占位。接入后端后 POST /api/v1/import {paths}
  if (!importPaths.value.trim()) {
    ElMessage.warning('请输入文件/文件夹路径')
    return
  }
  submitting.value = true
  // TODO(backend): post('/import', { paths: parsePaths() })
  setTimeout(() => {
    submitting.value = false
    importVisible.value = false
    ElMessage.success('导入任务已创建（骨架占位，未实际入库）')
    router.push('/tasks')
  }, 400)
}
</script>

<template>
  <el-header class="top-bar">
    <div class="left">
      <el-button type="primary" :icon="Plus" @click="importVisible = true">导入</el-button>
    </div>
    <div class="right">
      <el-button :icon="List" text @click="router.push('/tasks')" title="任务中心" />
      <el-button :icon="Sunny" text title="主题切换（骨架占位）" />
    </div>

    <el-dialog v-model="importVisible" title="导入图片" width="560px">
      <el-form label-width="90px">
        <el-form-item label="路径">
          <el-input
            v-model="importPaths"
            type="textarea"
            :rows="4"
            placeholder="支持文件夹或文件，每行一个&#10;例如：D:/Pictures 或 D:/a/1.png"
          />
        </el-form-item>
        <el-form-item label="导入方式">
          <el-radio-group v-model="importMode">
            <el-radio value="move">移动进库（源位置清空）</el-radio>
            <el-radio value="copy">复制进库（保留源文件）</el-radio>
          </el-radio-group>
        </el-form-item>
        <el-alert type="warning" :closable="false" show-icon
          title="移动进库后原位置文件将被移走，请确认路径无误" />
      </el-form>
      <template #footer>
        <el-button @click="importVisible = false">取消</el-button>
        <el-button type="primary" :loading="submitting" @click="onImportConfirm">开始导入</el-button>
      </template>
    </el-dialog>
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
</style>
