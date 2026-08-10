import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface SettingsState {
  libraryDir: string
  saucenaoKey: string
  saucenaoSimilarity: number
  tagThreshold: number
  dedupHamming: number
  aestheticModel: string
  /** 0 = 不自动清空 */
  recycleDays: number
  sidecarEnabled: boolean
  cnDictEnabled: boolean
  darkMode: 'auto' | 'light' | 'dark'
}

const defaults: SettingsState = {
  libraryDir: 'data/library',
  saucenaoKey: '',
  saucenaoSimilarity: 75,
  tagThreshold: 0.5,
  dedupHamming: 8,
  aestheticModel: 'trojblue/distill-q-align-aesthetic-siglip2-base',
  recycleDays: 0,
  sidecarEnabled: false,
  cnDictEnabled: false,
  darkMode: 'auto',
}

/** 设置状态（骨架阶段为前端本地默认值，接入后端后读写 /api/v1/settings） */
export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<SettingsState>({ ...defaults })

  function update(patch: Partial<SettingsState>) {
    settings.value = { ...settings.value, ...patch }
  }

  function reset() {
    settings.value = { ...defaults }
  }

  return { settings, update, reset }
})
