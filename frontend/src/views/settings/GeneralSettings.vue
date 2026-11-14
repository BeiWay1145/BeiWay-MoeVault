<script setup lang="ts">
import { useSettingsStore } from '@/stores/settings'

const settings = useSettingsStore()
</script>

<template>
  <el-form label-width="160px" style="max-width: 720px">
    <el-form-item label="库目录">
      <el-input v-model="settings.settings.library_dir" placeholder="data/library" />
    </el-form-item>
    <el-form-item label="关闭时最小化到托盘">
      <el-switch v-model="settings.settings.close_to_tray" active-text="开启" inactive-text="关闭" />
      <span class="hint">开启后点关闭按钮最小化到系统托盘（后台任务继续），托盘图标可恢复/退出</span>
    </el-form-item>
    <el-form-item label="瀑布流列数">
      <el-select v-model="settings.settings.waterfall_columns" style="width: 200px">
        <el-option label="自动（传统瀑布流）" value="auto" />
        <el-option label="2 列" value="2" />
        <el-option label="3 列" value="3" />
        <el-option label="4 列" value="4" />
        <el-option label="5 列" value="5" />
        <el-option label="6 列" value="6" />
      </el-select>
      <span class="hint">自动=传统瀑布流（紧密错落、按列填充）；固定列=网格按行排（无空隙）。影响图库/搜索</span>
    </el-form-item>
    <el-form-item label="侧边栏悬停展开">
      <el-switch v-model="settings.settings.sidebar_hover_expand" active-text="开启" inactive-text="关闭" />
      <span class="hint">侧边栏收起后，鼠标悬停自动展开（默认开启）</span>
    </el-form-item>
    <el-form-item label="分页模式">
      <el-switch v-model="settings.settings.pagination_enabled" active-text="开启" inactive-text="关闭" />
      <span class="hint">开启后图库按每页固定条数分页（每页条数在图库页右下角设置），关闭则一次加载全部</span>
    </el-form-item>
    <el-form-item label="默认导入方式">
      <el-radio-group v-model="settings.settings.import_default_mode">
        <el-radio value="move">移动进库</el-radio>
        <el-radio value="copy">复制进库</el-radio>
      </el-radio-group>
      <span class="hint">拖入导入/手动导入时的默认模式（确认框内仍可临时切换）</span>
    </el-form-item>
    <el-form-item label="导出后打开目录">
      <el-checkbox v-model="settings.settings.export_open_explorer_batch">批量导出后自动打开资源管理器</el-checkbox>
      <br />
      <el-checkbox v-model="settings.settings.export_open_explorer_manual">手动导出后自动打开资源管理器</el-checkbox>
      <span class="hint">导出完成后自动打开资源管理器到目标目录（默认关闭；也可点击成功提示跳转）</span>
    </el-form-item>
    <el-form-item label="导出默认目录">
      <el-input v-model="settings.settings.export_default_dir" placeholder="留空 = 自动探测下载目录" style="width: 420px" />
      <span class="hint">批量导出弹窗的目标目录默认值（如 D:\Downloads）</span>
    </el-form-item>
    <el-form-item label="预加载图片张数">
      <el-input-number v-model="settings.settings.preload_count" :min="0" :max="5" />
      <span class="hint">详情页切换图片时预加载前后各 N 张原图（0=关闭，默认 4），减少切换闪灰</span>
    </el-form-item>
  </el-form>
</template>
