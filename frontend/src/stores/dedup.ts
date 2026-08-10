import { defineStore } from 'pinia'
import { ref } from 'vue'

/** 查重相关状态（骨架占位，接入后端后由 /api/v1/dedup/* 驱动） */
export const useDedupStore = defineStore('dedup', () => {
  const groupCount = ref(0)
  const redundantCount = ref(0)
  const involvedImages = ref(0)

  // mock 初始值，便于 UI 展示角标
  function loadMock() {
    groupCount.value = 24
    redundantCount.value = 37
    involvedImages.value = 61
  }

  return { groupCount, redundantCount, involvedImages, loadMock }
})
