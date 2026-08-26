import { defineStore } from 'pinia'
import { ref } from 'vue'
import { get, put, post, del } from '@/api/client'

export interface SauceKeyConfig {
  name: string
  key_masked: string
  tier: string
  has_key: boolean
}

export interface SettingsState {
  saucenao_min_sim: number
  tag_threshold: number
  tagger_model_dir: string
  tagger_model_name: string
  aesthetic_model: string
  /** 推理设备：auto/cuda/cpu（打标与美学各一） */
  tagger_device: string
  aesthetic_device: string
  dedup_hamming: number
  sidecar_enabled: boolean
  cn_dict_enabled: boolean
  recycle_days: number
  library_dir: string
  /** E6: 画廊/搜索分页模式（默认关闭） */
  pagination_enabled: boolean
  page_size: number
  /** 关闭窗口时最小化到托盘（默认关闭=正常退出） */
  close_to_tray: boolean
  /** 瀑布流列数：auto/2/3/4/5/6（auto=传统瀑布流，固定=网格按行） */
  waterfall_columns: string
  /** 启动时清空旧日志（默认开启） */
  log_clear_on_start: boolean
  /** 侧边栏悬停自动展开（默认开启） */
  sidebar_hover_expand: boolean
  /** 详情页预加载图片张数（前后各 N 张，默认 2，0=关闭） */
  preload_count: number
}

const defaults: SettingsState = {
  saucenao_min_sim: 75,
  tag_threshold: 0.5,
  // 空 = 自动探测（推荐）：服务按 项目内 models/tagger → 旧位置 → 自定义 顺序自动定位
  tagger_model_dir: '',
  tagger_model_name: '自动探测',
  aesthetic_model: 'trojblue/distill-q-align-aesthetic-siglip2-base',
  tagger_device: 'auto',
  aesthetic_device: 'auto',
  dedup_hamming: 8,
  sidecar_enabled: false,
  cn_dict_enabled: false,
  recycle_days: 0,
  library_dir: 'data/library',
  pagination_enabled: false,
  page_size: 50,
  close_to_tray: false,
  waterfall_columns: 'auto',
  log_clear_on_start: true,
  sidebar_hover_expand: true,
  preload_count: 4,
}

/** 设置状态：读写 /api/v1/settings，含多 key 管理。 */
export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<SettingsState>({ ...defaults })
  const loaded = ref(false)

  async function load() {
    try {
      const s = await get<Record<string, unknown>>('/settings')
      settings.value = {
        ...defaults,
        saucenao_min_sim: s.saucenao_min_sim != null ? Number(s.saucenao_min_sim) : defaults.saucenao_min_sim,
        tag_threshold: s.tag_threshold != null ? Number(s.tag_threshold) : defaults.tag_threshold,
        tagger_model_dir: String(s.tagger_model_dir ?? defaults.tagger_model_dir),
        tagger_model_name: String(s.tagger_model_name ?? defaults.tagger_model_name),
        aesthetic_model: String(s.aesthetic_model ?? defaults.aesthetic_model),
        tagger_device: String(s.tagger_device ?? defaults.tagger_device),
        aesthetic_device: String(s.aesthetic_device ?? defaults.aesthetic_device),
        dedup_hamming: s.dedup_hamming != null ? Number(s.dedup_hamming) : defaults.dedup_hamming,
        sidecar_enabled: s.sidecar_enabled === true || s.sidecar_enabled === 'true',
        cn_dict_enabled: s.cn_dict_enabled === true || s.cn_dict_enabled === 'true',
        recycle_days: s.recycle_days != null ? Number(s.recycle_days) : defaults.recycle_days,
        library_dir: String(s.library_dir ?? defaults.library_dir),
        pagination_enabled: s.pagination_enabled === true || s.pagination_enabled === 'true',
        page_size: s.page_size != null ? Number(s.page_size) : defaults.page_size,
        close_to_tray: s.close_to_tray === true || s.close_to_tray === 'true',
        waterfall_columns: String(s.waterfall_columns ?? defaults.waterfall_columns),
        log_clear_on_start: s.log_clear_on_start === true || s.log_clear_on_start !== 'false',
        sidebar_hover_expand: s.sidebar_hover_expand === true || s.sidebar_hover_expand !== 'false',
        preload_count: s.preload_count != null ? Number(s.preload_count) : defaults.preload_count,
      }
      loaded.value = true
    } catch {
      loaded.value = true // 后端不可用时用默认值
    }
  }

  async function save() {
    await put('/settings', {
      saucenao_min_sim: String(settings.value.saucenao_min_sim),
      tag_threshold: String(settings.value.tag_threshold),
      tagger_model_dir: settings.value.tagger_model_dir,
      tagger_model_name: settings.value.tagger_model_name,
      aesthetic_model: settings.value.aesthetic_model,
      tagger_device: settings.value.tagger_device,
      aesthetic_device: settings.value.aesthetic_device,
      dedup_hamming: String(settings.value.dedup_hamming),
      sidecar_enabled: String(settings.value.sidecar_enabled),
      cn_dict_enabled: String(settings.value.cn_dict_enabled),
      recycle_days: String(settings.value.recycle_days),
      library_dir: settings.value.library_dir,
      pagination_enabled: String(settings.value.pagination_enabled),
      page_size: String(settings.value.page_size),
      close_to_tray: String(settings.value.close_to_tray),
      waterfall_columns: settings.value.waterfall_columns,
      log_clear_on_start: String(settings.value.log_clear_on_start),
      sidebar_hover_expand: String(settings.value.sidebar_hover_expand),
      preload_count: String(settings.value.preload_count),
    })
  }

  function reset() {
    settings.value = { ...defaults }
  }

  return { settings, loaded, load, save, reset }
})
