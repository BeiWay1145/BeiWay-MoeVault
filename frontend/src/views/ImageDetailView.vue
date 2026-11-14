<script setup lang="ts">
import { computed, h, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox, ElNotification } from 'element-plus'
import { ArrowDown, ArrowRight, ArrowLeft } from '@element-plus/icons-vue'
import { displayTagName } from '@/utils/tagNormalize'
import { useLibraryStore, originalUrl } from '@/stores/library'
import { findScrollContainer } from '@/utils/scrollTarget'
import { useTaskStore } from '@/stores/tasks'
import { useSettingsStore } from '@/stores/settings'
import { get, post, put, del } from '@/api/client'
import { reportLog } from '@/api/log'
import { fmtBytes } from '@/utils/format'
import { groupTags, TAG_GROUP_DEFS, type DetailTag } from '@/constants/tagGroups'
import TagEditPanel from '@/components/detail/TagEditPanel.vue'
import SauceCompareDialog from '@/components/detail/SauceCompareDialog.vue'

const route = useRoute()
const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()
const settingsStore = useSettingsStore()

const image = computed(() => library.images.find((i) => i.id === Number(route.params.id)))
const tags = ref<DetailTag[]>([])
const aiInfo = ref<string | null>(null)
const aiTags = ref<string[]>([])
const aiChecked = ref(false)
const stageRef = ref<HTMLElement | null>(null)
/** 全屏查看模式（点击图片进入；叉号/ESC 退出；左右键切换）。 */
const fullscreen = ref(false)

/** 上一张图的 src（新图加载完成前保留显示）。 */
const prevSrc = ref<string | undefined>(undefined)
/** 图片替换后强制绕过浏览器缓存（URL 加时间戳）。 */
const cacheBust = ref(0)

const originalSrc = computed(() => {
  if (!image.value) return undefined
  const base = originalUrl(image.value.id)
  return cacheBust.value ? `${base}?t=${cacheBust.value}` : base
})

/** 改进2：切换图片时旧图保留，新图加载完成后淡入（避免灰色闪烁）。 */
const imgLoaded = ref(true)
/** 当前已加载完成显示的图片 id（用于追踪旧图，避免 AB 串图）。 */
const loadedImgId = ref<number | null>(null)
function onStageImgLoad() {
  imgLoaded.value = true
  if (image.value) loadedImgId.value = image.value.id
}
/** 新图加载失败：也恢复显示（避免一直停留在旧图）。 */
function onStageImgError() {
  imgLoaded.value = true
  if (image.value) loadedImgId.value = image.value.id
}
// 路由参数变化（切换图片）→ 新图未加载完成前保留旧图（旧图 = 已加载的那张）
watch(
  () => route.params.id,
  (newId, oldId) => {
    // 视觉改进1：切换图片时**重算目标缩略图坐标**。
    //
    // 为什么必须重算：图库中每张缩略图的位置各不相同（网格/瀑布流的列位置不同），
    // 若沿用最初那张的 thumbRect，翻页后返回会飞向错误位置
    // ——用户反馈的"错位到左边"正是跨列时飞到了原图所在列。
    //
    // 可行原因：图库被 keep-alive 缓存，其 DOM 仍在文档中，
    // 可通过 querySelector 找到对应缩略图并读取其当前坐标
    // （此时详情页在前台，图库隐藏，但元素仍有布局坐标可读）。
    const idNum = Number(newId)
    if (library.detailPos && Number.isFinite(idNum) && library.detailPos.from !== 'imports') {
      // 优先用布局快照纯计算（可靠，不受隐藏态影响）；算不出则保持原值
      const computed = lookupTargetRect(idNum)
      library.detailPos = {
        ...library.detailPos,
        imageId: idNum,
        thumbRect: computed ?? library.detailPos.thumbRect,
      }
    }
    // 已加载完成的图（loadedImgId）才是旧图；仅记住真正的已显示图
    prevSrc.value = loadedImgId.value != null ? originalUrl(loadedImgId.value) : undefined
    imgLoaded.value = false
    // 预加载前后 N 张原图（浏览器缓存命中 → 切换时立即显示，减少闪灰）
    preloadAround()
  },
)

// ---- 全屏大图（原生 img + opacity 叠加，杜绝 el-image 灰色占位） ----
/** 全屏当前显示的原图 src。 */
const fsSrc = ref<string | undefined>(undefined)
/** 全屏新图是否加载完成（加载中旧图保持显示，无灰色）。 */
const fsImgReady = ref(true)
/** 全屏切换时保留的旧图 src（新图 onload 前显示）。 */
const fsPrevSrc = ref<string | undefined>(undefined)

function openFullscreen() {
  fullscreen.value = true
  fsSrc.value = originalSrc.value
  fsImgReady.value = true
  fsPrevSrc.value = undefined
}
/** 全屏切图：新图就位前旧图保持显示（opacity 叠加），onload 后淡入。 */
function fsGoto(delta: number) {
  const ids = orderedIds.value
  if (ids.length === 0 || indexInList.value < 0) return
  const nextId = ids[(indexInList.value + delta + ids.length) % ids.length]
  const newSrc = originalUrl(nextId)
  // 保留当前图作为旧图
  fsPrevSrc.value = fsSrc.value
  fsImgReady.value = false
  // 更新当前图 + 路由（组件 watch 会触发普通视图过渡）
  router.replace(`/library/${nextId}`)
  fsSrc.value = newSrc
}

/** 预加载当前图前后各 N 张原图（用 Image 对象触发浏览器缓存）。 */
function preloadAround() {
  const ids = orderedIds.value
  const idx = indexInList.value
  const n = settingsStore.settings.preload_count || 0
  if (ids.length === 0 || idx < 0 || n <= 0) return
  const cur = Number(route.params.id)
  for (let d = 1; d <= n; d++) {
    for (const sign of [-1, 1]) {
      const target = ids[(idx + sign * d + ids.length) % ids.length]
      if (target !== cur) {
        const img = new Image()
        img.src = originalUrl(target)
      }
    }
  }
}

// E3: 标签按 danbooru 分类（画师/系列/角色/常规）。
// 分组口径由 constants/tagGroups.ts 统一提供，标签编辑弹窗与本页共用一份，
// 避免两处各自维护分类规则后逐渐漂移。
const tagGroups = computed(() => groupTags(tags.value))
const tagGroupDefs = TAG_GROUP_DEFS
const hasAnyTags = computed(() => tags.value.length > 0)

/** 增强2：有序导航 id 列表——浏览上下文（来源目录组/筛选结果/搜索结果）优先，
 *  当前图不在上下文（直达 URL/刷新）时回退全局库顺序。 */
const orderedIds = computed<number[]>(() => {
  const ctx = library.viewerContext
  const id = Number(route.params.id)
  if (ctx && ctx.ids.length > 0 && ctx.ids.includes(id)) return ctx.ids
  return library.images.map((i) => i.id)
})

// 上一张/下一张（在浏览上下文内循环）
const indexInList = computed(() => orderedIds.value.indexOf(Number(route.params.id)))

function gotoImage(delta: number) {
  const ids = orderedIds.value
  if (ids.length === 0 || indexInList.value < 0) return
  const next = ids[(indexInList.value + delta + ids.length) % ids.length]
  // 组件内切换：router.replace 不推入历史（返回时直接回图库而非逐张回退）
  router.replace(`/library/${next}`)
}

// 键盘左右键切换 + Del 删除 + ESC 退出全屏
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    fullscreen.value = false
    return
  }
  if (e.key === 'ArrowLeft') gotoImage(-1)
  else if (e.key === 'ArrowRight') gotoImage(1)
  else if (e.key === 'Delete' || e.key === 'Del') recycle()
}

/** 点击图片区域进入全屏。 */
function enterFullscreen() {
  openFullscreen()
}

// 路由参数变化（左右切换/URL 直达）时刷新标签等详情数据（BUG1 修复）
watch(
  () => route.params.id,
  () => {
    tags.value = []
    aiInfo.value = null
    aiTags.value = []
    aiChecked.value = false
    loadDetail()
  },
)

async function loadDetail() {
  const id = Number(route.params.id)
  if (!image.value) {
    await library.fetchImages(500).catch(() => {})
  }
  // 标签
  try {
    const t = await get<{ tags: Array<{ tag_id: number; name: string; name_cn: string | null; category: string; source: string; image_count: number }> }>(
      `/images/${id}/tags`,
    )
    tags.value = t.tags
    aiTags.value = t.tags.filter((x) => x.source === 'ai').map((x) => x.name)
    aiChecked.value = !!image.value?.isAi
  } catch {
    tags.value = []
  }
  // 已存的 AI 信息
  aiChecked.value = !!image.value?.isAi
}

async function readAiInfo() {
  const id = Number(route.params.id)
  try {
    const r = await post<{ ok: boolean; is_ai: boolean; metadata: string | null; prompt?: string | null; negative_prompt?: string | null; tags?: string[] }>(`/images/${id}/ai-info`)
    // 增强4.2：图片已带 AI 标签时，即使本次未读到元信息也不清除 AI 状态
    if (image.value?.isAi) {
      aiChecked.value = true
    } else {
      aiChecked.value = r.is_ai
    }
    aiInfo.value = r.metadata
    if (r.tags && r.tags.length > 0) {
      aiTags.value = r.tags
      ElMessage.success(`已提取 ${r.tags.length} 个 AI 生图标签`)
    } else if (aiChecked.value) {
      ElMessage.success('已标记为 AI 图片（无有效 prompt 标签）')
    } else {
      ElMessage.info('未检测到 AI 生成元信息')
    }
    // 刷新标签列表
    await loadDetail()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// 增强4.1：手动标记/取消标记 AI（toggle）
async function toggleAiMark() {
  if (!image.value) return
  const next = !aiChecked.value
  try {
    await post(`/images/${image.value.id}/mark-ai`, { ai: next })
    aiChecked.value = next
    // 同步本地列表中的 isAi（图库筛选/角标即时生效）
    const it = library.images.find((i) => i.id === image.value!.id)
    if (it) it.isAi = next
    ElMessage.success(next ? '已标记为 AI 生成图片' : '已取消 AI 生成标记')
    await loadDetail()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// 放入回收站：无提示；删除后跳到上下文内的上一张，无则下一张，无则回图库
async function recycle() {
  if (!image.value) return
  const id = image.value.id
  const ids = [...orderedIds.value]
  const idx = ids.indexOf(id)
  try {
    await post(`/images/${id}/recycle`, { reason: 'manual' })
    // 从本地列表与浏览上下文移除，避免 computed 失效
    library.removeImageById(id)
    library.removeFromViewerContext(id)
    // 目标：优先上下文内上一张，无则下一张，无则回图库
    let targetId: number | undefined
    if (idx > 0) targetId = ids[idx - 1]
    else if (idx === 0 && ids.length > 1) targetId = ids[1]
    if (targetId != null) {
      router.push(`/library/${targetId}`)
    } else {
      router.back()
    }
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 缩回过渡进行中（避免重复触发）。 */
const zoomingOut = ref(false)

/**
 * 查表取得目标缩略图的矩形（进入详情时由图库采集的真实坐标）。
 *
 * 为什么查表而不计算：瀑布流各列独立堆叠、每列高度不同，
 * 用行列公式复现会有明显偏差（曾实测偏约 1/4 卡片高度）。
 * 采集的是渲染结果的**真实坐标**，因此逐像素准确。
 *
 * 注意：表中坐标是"采集时的视口坐标"，需按滚动差换算到当前视口。
 */
function lookupTargetRect(imageId: number): { x: number; y: number; w: number; h: number } | null {
  const L = library.listLayout
  if (!L) return null
  const r = L.rects[imageId]
  if (!r) return null
  // **直接返回采集到的视口坐标，不做滚动换算。**
  //
  // 为什么：rects 存的就是 getBoundingClientRect() 的视口坐标（已含当时的滚动），
  // 而返回时 restorePos 会把滚动**恢复到同一个值**，两者基准一致，直接用即正确。
  //
  // 此前的错误：这里再减了一次 (scrollNow - L.scrollTop)。
  // 但在详情页里探测滚动容器会得到错误结果（详情页没有 .wall-container，
  // 探测退化为 .app-main，其 scrollTop 恒为 0），于是算出 y + 1700 的严重偏移。
  return { x: r.x, y: r.y, w: r.w, h: r.h }
}

/**
 * 视觉改进1：返回图库前，让详情页大图"缩回"它在图库缩略图中的位置。
 *
 * 设计（相比上一版的修正）：
 * - **在详情页内就地播放**，动画结束后才 `router.push` ——
 *   上一版是"先导航、再由图库从零构建浮层"，期间真实缩略图已渲染，
 *   两者叠加反而造成闪烁，且目标元素常常还没就绪（改进无效果）。
 * - 目标位置需要缩略图的屏幕坐标：图库处于 keep-alive 中，
 *   其 DOM 仍在文档里（只是被隐藏），因此**可以提前查到**目标矩形。
 * - 过渡用 transform（GPU 加速、不影响布局），结束后导航，视觉上连续。
 */
async function goBack() {
  const pos = library.detailPos
  const from = pos?.from
  const img = image.value
  const stage = stageRef.value
  // 动画载体选择（三层兜底，避免取到隐藏/零尺寸元素导致动画被静默跳过）：
  // 1) 当前图的 <img>（.el-image__inner）—— 真正可见、有实际尺寸，动画效果最好
  // 2) 当前图的 el-image 容器（.stage-img，排除隐藏的旧图 .prev-img）
  // 3) 整个 .stage
  // 注意：.stage 里同时存在旧图（v-show 隐藏）与当前图，querySelector 会返回文档顺序靠前者，
  // 若取到隐藏的旧图则 getBoundingClientRect() 全为 0 → 动画直接跳过。
  const stageImgs = stage ? Array.from(stage.querySelectorAll<HTMLElement>('.stage-img')) : []
  const curWrap = stageImgs.find((n) => !n.classList.contains('prev-img')) ?? stageImgs[0] ?? null
  const curInner = curWrap?.querySelector<HTMLElement>('.el-image__inner') ?? null
  const el =
    (curInner && curInner.getBoundingClientRect().width > 0 ? curInner : null) ??
    (curWrap && curWrap.getBoundingClientRect().width > 0 ? curWrap : null) ??
    stage

  // 诊断日志（定位动画未播放的原因，确认后移除）
  const hasThumbRectOk = !!pos?.thumbRect
  reportLog(
    '[flyback] goBack ' +
      JSON.stringify({
        from,
        hasImg: !!img,
        hasEl: !!el,
        elCls: el?.className,
        hasThumbRect: !!pos?.thumbRect,
        rect: pos?.thumbRect,
        zoomingOut: zoomingOut.value,
      }),
    hasThumbRectOk ? 'info' : 'warn',
  )

  // 主目录结构不同（分组展开），暂不播放缩略回退动画
  const canAnimate =
    from !== 'imports' && !!img && !!el && !!pos?.thumbRect && !zoomingOut.value
  if (!canAnimate) {
    if (from === 'imports') router.push('/imports')
    else router.push('/library')
    return
  }

  const fromRect = el!.getBoundingClientRect()
  if (fromRect.width === 0) {
    router.push('/library')
    return
  }

  // 目标矩形：**优先查表**（进入详情时由图库采集的真实坐标）。
  //
  // 此前这里直接读 pos.thumbRect —— 那是**最初点开那张**的坐标，
  // 详情页翻页后返回就会飞向错误位置（落点偏差的直接原因）。
  // 现在统一走 lookupTargetRect：它按当前显示的图片 id 查真实坐标表，
  // 覆盖"未翻页"与"翻页后"两种情况。
  const looked = lookupTargetRect(img!.id)
  const fallback = pos!.thumbRect!
  // 同样直接使用记录的视口坐标（返回时滚动会恢复到同一位置，基准一致）
  const toRect = looked ?? { x: fallback.x, y: fallback.y, w: fallback.w, h: fallback.h }

  zoomingOut.value = true

  // ---- 时序设计（按需求）：先让详情页 UI 消失，再让图片飞向图库缩略图 ----
  //
  // 为什么这样：图片属于详情页，只要详情页还在，其上方就叠着导航栏/顶栏/关闭按钮等 UI，
  // 图片一旦移出舞台范围就会被这些 UI 盖住（此前反馈的"图移到了 UI 显示层下面"）。
  // 因此顺序改为：
  //   1. 先把飞行层放到 body（z-index 最高，独立于任何页面）
  //   2. 立即导航到图库 —— 详情页 UI 随之消失，露出图库
  //   3. 等图库渲染完成（下一帧起）再启动过渡，图片边移动边缩小到缩略图位置
  //   4. 过渡结束移除飞行层，此时真实缩略图已就位，视觉无缝
  const src =
    (el as HTMLImageElement).src ??
    (el.querySelector('img') as HTMLImageElement | null)?.src ??
    ''
  // 计算图片在舞台容器内**实际渲染**的尺寸（contain 缩放后的内容框）。
  // 关键：fromRect 是 el-image 元素（撑满 .stage 的容器）的尺寸，
  // 而可见的图只占其中一部分；若用容器尺寸做缩放基准，
  // 会导致"大图飞回后偏小、小图偏大"（用户反馈的尺寸不匹配）。
  const stageW = fromRect.width
  const stageH = fromRect.height
  const natW = img!.width > 0 ? img!.width : stageW
  const natH = img!.height > 0 ? img!.height : stageH
  const contentScale = Math.min(stageW / natW, stageH / natH)
  const visW = natW * contentScale
  const visH = natH * contentScale
  // 图片内容框在舞台中的居中偏移
  const offX = (stageW - visW) / 2
  const offY = (stageH - visH) / 2

  const ghost = document.createElement('img')
  ghost.src = src
  ghost.style.cssText = [
    'position:fixed',
    `left:${fromRect.left + offX}px`,
    `top:${fromRect.top + offY}px`,
    `width:${visW}px`,
    `height:${visH}px`,
    'object-fit:fill',
    'z-index:99999',
    'pointer-events:none',
    'border-radius:8px',
    'will-change:transform,opacity',
    'transition:transform .38s cubic-bezier(.4,0,.2,1),opacity .38s ease',
    'transform-origin:center center',
    'transform:translate(0,0) scale(1)',
    'opacity:1',
  ].join(';')
  document.body.appendChild(ghost)

  // 计算位移与缩放（以中心点对齐目标缩略图中心）
  // 缩放与位移都以"图片可见内容框"为基准（而非容器）
  const dx = toRect.x + toRect.w / 2 - (fromRect.left + offX + visW / 2)
  const dy = toRect.y + toRect.h / 2 - (fromRect.top + offY + visH / 2)
  const scale = Math.max(0.02, toRect.w / visW)

  // 第 2 步：立即导航 —— 详情页 UI 消失，露出图库
  router.push('/library')

  // 第 3 步：等图库"滚动位置恢复完成"后再启动过渡。
  // 图库在 onActivated 末尾派发 moevault:library-ready，此时目标缩略图已在最终位置；
  // 若不等待而直接用固定的两帧延迟，滚动尚未完成会导致落点偏移。
  // 另设超时兜底：事件因异常未到达时，350ms 后仍然启动，避免图片卡住不动。
  reportLog(
    '[flyback] 启动飞行 ' +
      JSON.stringify({
        imgId: img!.id,
        fromX: Math.round(fromRect.left),
        fromY: Math.round(fromRect.top),
        toX: Math.round(toRect.x),
        toY: Math.round(toRect.y),
        toW: Math.round(toRect.w),
        dx: Math.round(dx),
        dy: Math.round(dy),
        scale: Number(scale.toFixed(3)),
        usedLookup: !!looked,
      }),
    'info',
  )

  let started = false
  const startFly = () => {
    if (started) return
    started = true
    window.removeEventListener('moevault:library-ready', startFly)
    requestAnimationFrame(() => {
      ghost.style.transform = `translate(${dx}px, ${dy}px) scale(${scale})`
      ghost.style.opacity = '0.3'
    })
  }
  window.addEventListener('moevault:library-ready', startFly, { once: true })
  window.setTimeout(startFly, 350)

  // 第 4 步：过渡结束移除飞行层（真实缩略图已就位，视觉无缝）
  let done = false
  const cleanup = () => {
    if (done) return
    done = true
    ghost.remove()
  }
  ghost.addEventListener('transitionend', cleanup, { once: true })
  // 兜底：过渡事件未触发时（元素被隐藏等）用定时器保证收尾
  window.setTimeout(cleanup, 520)
}

// 手动打标（BUG3 任务化）：加入打标队列，进度见任务中心
async function manualTag() {
  if (!image.value) return
  try {
    await taskStore.enqueueTag([image.value.id])
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// 增强5：美学处理（Q-Align 评分任务）
async function rescoreAesthetic() {
  if (!image.value) return
  try {
    await taskStore.enqueueAesthetic([image.value.id])
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// 增强5：尝试溯源（SauceNAO 任务）；增强3：完成后比对网络原图大小 → 弹一键替换提示
async function trySauce() {
  if (!image.value) return
  const id = image.value.id
  const localSize = image.value.sizeBytes
  try {
    await taskStore.enqueueSauce([id], false, false, async () => {
      await checkSauceReplace(id, localSize)
    })
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 增强3（单张溯源）：溯源完成后比对网络原图与本地图大小；差异 ≥2 倍 → 右上角弹一键替换提示。 */
async function checkSauceReplace(id: number, localSize: number) {
  try {
    const r = await get<{
      ok: boolean
      page_url: string
      info: { width: number | null; height: number | null; size_bytes: number | null; file_url: string | null }
    }>(`/images/${id}/source-info`)
    const netSize = r.info.size_bytes ?? 0
    if (!r.info.file_url || netSize < 2 * localSize) return // 差异不足 → 不提示
    // 刷新详情（溯源已更新来源链接）
    await loadDetail()
    ElNotification({
      title: '发现更清晰的原图',
      message: h('div', null, [
        h('p', null, `网络原图 ${fmtBytes(netSize)} ≈ ${localSize > 0 ? (netSize / localSize).toFixed(1) : '?'} 倍于本地图（${fmtBytes(localSize)}）`),
        h(
          'button',
          {
            style:
              'margin-top:6px;padding:4px 12px;background:var(--el-color-primary);color:#fff;border:none;border-radius:4px;cursor:pointer',
            onClick: () => doReplaceFromNet(id),
          },
          '一键替换原图',
        ),
      ]),
      type: 'warning',
      duration: 0,
      onClick: () => doReplaceFromNet(id),
    })
  } catch {
    /* 静默：比对失败不影响溯源结果 */
  }
}

/** 增强3：执行替换（下载网络原图替换库内文件，旧文件移入回收站）。 */
async function doReplaceFromNet(id: number) {
  try {
    const r = await get<{ ok: boolean; page_url: string; info: { file_url: string | null } }>(`/images/${id}/source-info`)
    if (!r.info.file_url) {
      ElMessage.error('无法获取网络图直链')
      return
    }
    await post(`/images/${id}/replace-from-url`, { url: r.info.file_url })
    ElMessage.success('已替换为网络原图（旧文件已移入回收站）')
    await loadDetail()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 手动导出：复制原图到导出默认目录（或下载目录）；设置开启时自动打开资源管理器。 */
async function exportImage() {
  if (!image.value) return
  try {
    const s = settingsStore
    let dir = s.settings.export_default_dir?.trim() ?? ''
    if (!dir) {
      const dl = await get<{ dir: string }>('/system/downloads-dir')
      dir = dl.dir
    }
    const r = await post<{ ok: boolean; count: number; target: string }>('/images/export', {
      ids: [image.value.id],
      target_dir: dir,
      bundle_name: null,
      pack: false,
      pack_format: 'zip',
      pack_level: 2,
      with_tags: false,
      recycle_after: false,
    })
    const target = r.target
    ElNotification({
      title: '导出完成',
      message: `${image.value.name} → ${target}（点击跳转目录）`,
      type: 'success',
      duration: 6000,
      onClick: () => openExplorer(target),
    })
    if (s.settings.export_open_explorer_manual) {
      openExplorer(target)
    }
  } catch (e) {
    // 回退：浏览器直接下载
    const a = document.createElement('a')
    a.href = originalSrc.value ?? ''
    a.download = image.value.name
    a.click()
    ElMessage.success('已通过浏览器下载')
  }
}

/** 在资源管理器中打开路径（手动导出跳转）。 */
async function openExplorer(path: string) {
  try {
    await post('/system/open-explorer', { path })
  } catch {
    /* 静默 */
  }
}

/** E2: 手动编辑溯源来源链接。 */
async function editSourceUrl() {
  if (!image.value) return
  const id = image.value.id
  const cur = image.value.sourceUrl ?? ''
  const { value } = await ElMessageBox.prompt('输入溯源来源链接（留空清除）', '编辑原图链接', {
    inputValue: cur.replace(/\.json$/, ''),
    inputPlaceholder: 'https://danbooru.donmai.us/posts/...',
  }).catch(() => ({ value: null as string | null }))
  if (value === null) return
  try {
    await put(`/images/${id}/source-url`, { url: value || null })
    const it = library.images.find((i) => i.id === id)
    if (it) it.sourceUrl = value || undefined
    ElMessage.success('原图链接已更新')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 改进1：重命名库内图片文件（保持哈希目录，冲突即失败）。 */
async function renameImage() {
  if (!image.value) return
  const id = image.value.id
  const { value } = await ElMessageBox.prompt('输入新文件名（含扩展名）', '重命名图片', {
    inputValue: image.value.name,
    inputPattern: /^[^\\/:*?"<>|]+$/,
    inputErrorMessage: '文件名含非法字符（\\ / : * ? " < > |）',
  }).catch(() => ({ value: null as string | null }))
  if (!value || value === image.value.name) return
  try {
    const r = await put<{ ok: boolean; rel_path: string }>(`/images/${id}/rename`, { name: value })
    const it = library.images.find((i) => i.id === id)
    if (it) it.name = decodeURIComponent((r.rel_path as string).split(/[\\/]/).pop() ?? value)
    ElMessage.success('已重命名')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 原图链接展示：去掉 .json 后缀（页面链接可点击跳转）。 */
const displaySourceUrl = computed(() =>
  image.value?.sourceUrl ? image.value.sourceUrl.replace(/\.json$/, '') : undefined,
)

/** BUG4：点击原图链接 → 桌面壳用系统浏览器打开；浏览器环境 fallback window.open。
 *  根因：window.__TAURI__ 需 withGlobalTauri:true（默认 false）→ 误判为非 Tauri 走了 window.open 被拦截。
 *  改用始终注入的 window.__TAURI_INTERNALS__ 直接 invoke opener。 */
async function openSourceUrl(url: string) {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: (cmd: string, args?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__
  console.log('[openSourceUrl]', { url, hasInternals: !!internals })
  if (internals?.invoke) {
    try {
      console.log('[openSourceUrl] invoke plugin:opener|open_url')
      await internals.invoke('plugin:opener|open_url', { url })
      console.log('[openSourceUrl] 调用成功')
      return
    } catch (e) {
      console.error('[openSourceUrl] invoke 失败:', e)
      ElMessage.error(`打开链接失败: ${(e as Error).message}`)
      return
    }
  }
  window.open(url, '_blank', 'noopener')
}

// ---- 对比原图 / 标签编辑：已拆分为独立组件 ----
// 对比原图弹窗 → components/detail/SauceCompareDialog.vue
// 标签编辑（含原始文本） → components/detail/TagEditPanel.vue
// 两者通过 v-model + 事件与父组件协作，父组件只保留「开关状态」与「保存后刷新」。
/** 对比原图弹窗开关（内容见 SauceCompareDialog）。 */
const compareVisible = ref(false)
/** 标签编辑弹窗开关（内容见 TagEditPanel）。 */
const editDialogVisible = ref(false)

function openCompare() {
  if (!image.value) return
  compareVisible.value = true
}

function openEditDialog() {
  editDialogVisible.value = true
}

/** 替换网络原图后：刷新库列表与详情，并绕过浏览器图片缓存。 */
async function onCompareReplaced() {
  await library.fetchImages(500)
  await loadDetail()
  prevSrc.value = originalSrc.value
  imgLoaded.value = false
  cacheBust.value++
}

// ---- 增强2：标签栏（左栏 danbooru 风格）----
/** 标签栏折叠状态（增强1：整栏收起；增强2：localStorage 持久化）。 */
const tagPanelCollapsed = ref(localStorage.getItem('moevault-detail-tag-collapsed') === '1')
/** 基本信息栏折叠状态（增强1：整栏收起；增强2：localStorage 持久化）。 */
const infoPanelCollapsed = ref(localStorage.getItem('moevault-detail-info-collapsed') === '1')
watch(tagPanelCollapsed, (v) => localStorage.setItem('moevault-detail-tag-collapsed', v ? '1' : '0'))
watch(infoPanelCollapsed, (v) => localStorage.setItem('moevault-detail-info-collapsed', v ? '1' : '0'))

/** 点标签 → 跳到图库页并只搜索该标签。 */
function jumpTag(name: string) {
  library.filter = { ...library.filter, tags: [name] }
  router.push('/library')
}

/** 一键复制标签（用户指定格式）：
 *  画师名,
 *  角色名,
 *  作品名,
 *  常规标签1,常规标签2,常规标签3,常规标签4,
 *  每类一行，行内逗号分隔，行尾逗号。 */
async function copyTags() {
  const lines: string[] = []
  for (const g of tagGroupDefs) {
    const list = tagGroups.value[g.key]
    if (list.length === 0) continue
    lines.push(`${list.map((t) => t.name).join(',')},`)
  }
  const text = lines.join('\n')
  try {
    await navigator.clipboard.writeText(text)
    ElMessage.success(`已复制 ${tags.value.length} 个标签到剪贴板`)
  } catch {
    ElMessage.error('复制失败（剪贴板不可用）')
  }
}

onMounted(async () => {
  await settingsStore.load().catch(() => {})
  loadDetail()
  window.addEventListener('keydown', onKeydown)
  preloadAround()
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
})

</script>

<template>
  <div v-if="image" class="detail">
    <!-- 增强1：左栏收起后的展开条 -->
    <div v-if="tagPanelCollapsed" class="panel-expand-bar left" title="展开标签栏" @click="tagPanelCollapsed = false">
      <el-icon><ArrowRight /></el-icon>
    </div>
    <!-- 增强2：左侧标签栏；BUG3：常驻元素 + 宽度过渡（动画生效、无瞬时偏移） -->
    <div class="tag-panel-wrap" :class="{ collapsed: tagPanelCollapsed }">
    <div class="tag-panel">
      <div class="tag-panel-header">
        <span class="tag-panel-title">
          标签（{{ tags.length }}）
        </span>
        <div class="tag-panel-actions">
          <el-button size="small" type="primary" plain @click="copyTags">一键复制</el-button>
          <el-button size="small" type="success" plain @click="openEditDialog">修改标签</el-button>
          <el-button size="small" text :title="'收起标签栏'" @click="tagPanelCollapsed = true">◀ 收起</el-button>
        </div>
      </div>
      <div class="tag-panel-sub">
        <el-button size="small" text type="primary" @click="readAiInfo">读取元数据</el-button>
        <el-button size="small" :type="aiChecked ? 'info' : 'warning'" text plain @click="toggleAiMark">
          {{ aiChecked ? '取消 AI 标记' : '标记为 AI' }}
        </el-button>
      </div>
      <div v-if="hasAnyTags" class="tag-panel-body">
        <div v-for="g in tagGroupDefs" :key="g.key" class="tag-sec">
          <div v-if="tagGroups[g.key as keyof typeof tagGroups].length > 0" class="tag-sec-label">
            {{ g.label }}（{{ tagGroups[g.key as keyof typeof tagGroups].length }}）
          </div>
          <div
            v-for="t in tagGroups[g.key as keyof typeof tagGroups]"
            :key="t.name"
            class="tag-line"
            :title="`${t.name}：${t.image_count} 张`"
            @click="jumpTag(t.name)"
          >
            <span class="tag-line-name">{{ displayTagName(t.name) }}</span>
            <span v-if="t.name_cn" class="tag-line-cn">[“{{ t.name_cn }}”]</span>
            <span class="tag-line-freq">{{ t.image_count }}</span>
          </div>
        </div>
      </div>
      <el-empty v-else description="暂无标签（可读取元数据）" :image-size="50" />
      <pre v-if="aiInfo" class="ai-info">{{ aiInfo }}</pre>
    </div>
    </div>

    <div class="viewer">
      <button class="nav-close" title="返回" @click="goBack">✕</button>
      <!-- 增强2：浏览上下文位置（来源目录组/筛选结果内的序号；无上下文时为全局库顺序） -->
      <div class="nav-pos" :title="library.viewerContext ? `浏览范围：${library.viewerContext.label}` : '浏览范围：全部图片'">
        {{ indexInList >= 0 ? indexInList + 1 : '?' }} / {{ orderedIds.length }}
        <span v-if="library.viewerContext" class="nav-pos-label">{{ library.viewerContext.label }}</span>
      </div>
      <button class="nav-arrow left" title="上一张" @click="gotoImage(-1)">‹</button>
      <div ref="stageRef" class="stage" @click="enterFullscreen">
        <!-- 旧图：新图加载完成前保持显示（v-show 不卸载；无旧图时隐藏） -->
        <el-image
          v-show="!imgLoaded && !!prevSrc"
          :src="prevSrc"
          fit="contain"
          class="stage-img prev-img"
        />
        <!-- 新图：始终渲染（v-show 而非 v-if，确保加载时能触发 @load），加载完成淡入 -->
        <el-image
          v-show="imgLoaded"
          :key="image.id"
          :src="originalSrc"
          fit="contain"
          class="stage-img"
          :class="{ 'img-fade-in': imgLoaded }"
          :preview-src-list="[]"
          @load="onStageImgLoad"
          @error="onStageImgError"
        >
          <template #error>
            <span class="placeholder-name">原图加载失败</span>
          </template>
        </el-image>
      </div>
      <button class="nav-arrow right" title="下一张" @click="gotoImage(1)">›</button>
    </div>

    <!-- 全屏查看模式（E1）：点击图片进入，叉号/ESC 退出，左右键切换 -->
    <Transition name="fs">
      <div v-if="fullscreen" class="fullscreen" @click="fullscreen = false">
        <button class="fs-close" title="退出全屏" @click.stop="fullscreen = false">✕</button>
        <button class="fs-arrow left" title="上一张" @click.stop="fsGoto(-1)">‹</button>
        <!-- 全屏大图：原生 img + opacity 交叉淡化（新图就位前旧图显示；就位后旧图淡出+新图淡入） -->
        <img
          v-if="fsPrevSrc"
          :key="'prev-' + fsPrevSrc"
          :src="fsPrevSrc"
          class="fs-img"
          :style="{ opacity: fsImgReady ? 0 : 1, transition: 'opacity 0.25s ease' }"
          alt=""
          @click.stop
        />
        <img
          :key="'cur-' + fsSrc"
          :src="fsSrc"
          class="fs-img"
          :style="{ opacity: fsImgReady ? 1 : 0, transition: 'opacity 0.25s ease' }"
          alt=""
          @load="fsImgReady = true"
          @click.stop
        />
        <button class="fs-arrow right" title="下一张" @click.stop="fsGoto(1)">›</button>
      </div>
    </Transition>

    <!-- 增强1：右栏收起后的展开条 -->
    <div v-if="infoPanelCollapsed" class="panel-expand-bar" title="展开右侧信息栏" @click="infoPanelCollapsed = false">
      <el-icon><ArrowLeft /></el-icon>
    </div>
    <!-- BUG3：右栏常驻 + 宽度过渡 -->
    <div class="panel-wrap" :class="{ collapsed: infoPanelCollapsed }">
    <div class="panel">
      <!-- 增强1：整栏收起按钮（右栏收起 → 图片区变大） -->
      <div class="panel-collapse-bar">
        <el-button size="small" text :title="'收起右侧信息栏'" @click="infoPanelCollapsed = true">▶ 收起</el-button>
      </div>
      <div class="panel-block">
        <div class="panel-title">
          基本信息
        </div>
        <el-descriptions :column="1" border>
        <el-descriptions-item label="文件名">
          <span class="file-name">{{ image.name }}</span>
          <el-button size="small" text type="primary" style="margin-left: 8px" @click="renameImage">重命名</el-button>
        </el-descriptions-item>
        <el-descriptions-item label="格式">{{ image.format?.toUpperCase() ?? (image.name.split('.').pop() ?? '').toUpperCase() }}</el-descriptions-item>
        <el-descriptions-item label="尺寸">{{ image.width }} × {{ image.height }}</el-descriptions-item>
        <el-descriptions-item label="清晰度">
          <span class="num-mono">{{ image.clarity.toFixed(1) }}</span>
        </el-descriptions-item>
        <el-descriptions-item label="美学分">
          <span class="num-mono">{{ image.aesthetic?.toFixed(1) ?? '—' }}</span>
        </el-descriptions-item>
        <el-descriptions-item label="导入时间">{{ new Date(image.importedAt * 1000).toLocaleDateString() }}</el-descriptions-item>
        <el-descriptions-item label="原图链接">
          <a
            v-if="displaySourceUrl"
            :href="displaySourceUrl"
            class="src-link"
            @click.prevent="openSourceUrl(displaySourceUrl)"
          >{{ displaySourceUrl }}</a>
          <span v-else class="muted">未溯源</span>
          <el-button size="small" text type="primary" style="margin-left: 8px" @click="editSourceUrl">编辑</el-button>
          <el-button v-if="displaySourceUrl" size="small" text type="success" style="margin-left: 4px" @click="openCompare">对比原图</el-button>
        </el-descriptions-item>
        <el-descriptions-item label="状态">
          <el-tag v-if="image.isRedundant" type="warning">冗余候选</el-tag>
          <el-tag v-else type="success">正常</el-tag>
          <el-tag v-if="aiChecked" type="primary" style="margin-left: 6px">AI 生成</el-tag>
          <el-tag v-else type="info" plain style="margin-left: 6px">非 AI 生成</el-tag>
          <el-tag v-if="image.sourceUrl || (image.source !== undefined && image.source !== 'local' && image.source !== '')" type="success" plain style="margin-left: 6px">已溯源</el-tag>
          <el-tag v-else-if="image.noAutoSauce || aiChecked" type="warning" plain style="margin-left: 6px">不可溯源</el-tag>
          <el-tag v-else type="info" plain style="margin-left: 6px">未溯源</el-tag>
          <el-tag v-if="tags.length > 0" type="success" plain style="margin-left: 6px">已打标</el-tag>
          <el-tag v-else type="warning" plain style="margin-left: 6px">未打标</el-tag>
        </el-descriptions-item>
      </el-descriptions>
      </div>

      <div class="panel-block">
        <div class="panel-title">操作</div>
        <el-button type="danger" plain @click="recycle">入回收站</el-button>
        <el-button @click="exportImage">导出</el-button>
        <el-button type="primary" plain @click="manualTag">手动打标</el-button>
        <el-button type="success" plain @click="rescoreAesthetic">美学处理</el-button>
        <el-button plain @click="trySauce">尝试溯源</el-button>
      </div>
    </div>
    </div>
  </div>

  <!-- 对比原图弹窗（已拆分为独立组件：拉取源站信息 + 左右对比 + 替换） -->
  <SauceCompareDialog
    v-model="compareVisible"
    :image-id="image?.id ?? null"
    :width="image?.width ?? null"
    :height="image?.height ?? null"
    :size-bytes="image?.sizeBytes ?? null"
    :original-src="originalSrc"
    @replaced="onCompareReplaced"
  />

  <!-- 标签编辑弹窗（已拆分为独立组件：编辑状态机 + 原始文本重建） -->
  <TagEditPanel
    v-model="editDialogVisible"
    :image-id="image?.id ?? null"
    :tags="tags"
    @saved="loadDetail()"
  />
  <el-empty v-if="!image" description="图片不存在或已删除" />
</template>

<style scoped>
.detail {
  display: flex;
  gap: 16px;
  height: 100%;
}
/* 增强2：左侧标签栏；BUG3：宽度过渡（动画生效、无瞬时偏移；内容 min-width 防挤压） */
.tag-panel-wrap {
  width: 300px;
  flex: none;
  overflow: hidden;
  transition: width 0.22s ease;
}
.tag-panel-wrap.collapsed {
  width: 0;
}
.tag-panel {
  width: 300px;
  min-width: 300px;
  height: 100%;
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-height: 0;
}
.tag-panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-bottom: 1px solid var(--el-border-color-lighter);
}
.tag-panel-title {
  font-size: 14px;
  font-weight: 600;
}
.tag-panel-actions {
  display: flex;
  gap: 6px;
}
.tag-panel-sub {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 12px;
  border-bottom: 1px solid var(--el-border-color-lighter);
}
.tag-panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 8px 12px;
}
.tag-sec {
  margin-bottom: 12px;
}
.tag-sec-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--el-text-color-secondary);
  margin-bottom: 4px;
}
.tag-line {
  display: flex;
  align-items: baseline;
  gap: 6px;
  padding: 3px 6px;
  border-radius: 6px;
  cursor: pointer;
  line-height: 1.5;
  font-size: 13px;
  user-select: none;
  flex-wrap: wrap;
}
.tag-line:hover {
  background: var(--el-fill-color-light);
}
.tag-line-name {
  color: var(--el-color-primary);
  word-break: break-all;
}
.tag-line-cn {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
.tag-line-freq {
  color: var(--el-text-color-placeholder);
  font-size: 12px;
  margin-left: auto;
}
.viewer {
  flex: 1;
  min-width: 0;
  position: relative;
  display: flex;
}
.stage {
  flex: 1;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #000;
  min-height: 320px;
  overflow: hidden;
}
.stage-img {
  width: 100%;
  height: 100%;
}
.stage-img :deep(.el-image__inner) {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
/* BUG1：el-image 加载中默认浅灰占位 → 改为透明（露出黑色 stage），暗色模式不刺眼 */
.stage-img :deep(.el-image__placeholder),
.stage-img :deep(.el-image__error) {
  background: transparent;
  color: var(--el-text-color-secondary);
}
/* 改进2：切换图片过渡——旧图保持，新图淡入 */
.stage-img.prev-img {
  position: absolute;
  inset: 0;
}
/* 切换图片：新图加载完成（v-show 显示）时淡入 */
.img-fade-in {
  animation: imgFade 0.25s ease;
}
@keyframes imgFade {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}
.nav-close {
  position: absolute;
  top: 12px;
  right: 12px;
  z-index: 10;
  width: 36px;
  height: 36px;
  border: none;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s;
}
.nav-close:hover {
  background: rgba(0, 0, 0, 0.7);
}
/* 增强2：上下文位置指示 */
.nav-pos {
  position: absolute;
  top: 18px;
  left: 16px;
  z-index: 10;
  padding: 4px 12px;
  border-radius: 14px;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 12px;
  line-height: 1.4;
  user-select: none;
}
.nav-pos-label {
  margin-left: 6px;
  opacity: 0.75;
}
.nav-arrow {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 10;
  width: 44px;
  height: 44px;
  border: none;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 30px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s;
}
.nav-arrow:hover {
  background: rgba(0, 0, 0, 0.7);
}
.nav-arrow.left {
  left: 16px;
}
.nav-arrow.right {
  right: 16px;
}
.placeholder-name {
  color: #888;
}
.src-link {
  color: var(--el-color-primary);
  word-break: break-all;
  font-size: 12px;
}
.muted {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
/* 全屏查看模式（E1） */
.fullscreen {
  position: fixed;
  inset: 0;
  z-index: 2000;
  background: rgba(0, 0, 0, 0.97);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: zoom-out;
}
.fs-img {
  width: 100%;
  height: 100%;
  cursor: zoom-out;
  object-fit: contain;
  position: absolute;
  inset: 0;
  transition: opacity 0.25s ease;
}
.fs-close {
  position: absolute;
  top: 16px;
  right: 16px;
  z-index: 10;
  width: 40px;
  height: 40px;
  border: none;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.18);
  color: #fff;
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s;
}
.fs-close:hover {
  background: rgba(255, 255, 255, 0.35);
}
.fs-arrow {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 10;
  width: 52px;
  height: 52px;
  border: none;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.14);
  color: #fff;
  font-size: 36px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s;
}
.fs-arrow:hover {
  background: rgba(255, 255, 255, 0.32);
}
.fs-arrow.left {
  left: 24px;
}
.fs-arrow.right {
  right: 24px;
}
.fs-enter-active,
.fs-leave-active {
  transition: opacity 0.2s ease;
}
.fs-enter-from,
.fs-leave-to {
  opacity: 0;
}
.panel-wrap {
  width: 400px;
  flex: none;
  overflow: hidden;
  transition: width 0.22s ease;
}
.panel-wrap.collapsed {
  width: 0;
}
.panel {
  width: 400px;
  min-width: 400px;
  height: 100%;
  overflow-y: auto;
}
/* 增强1：右栏收起条/展开条 */
.panel-collapse-bar {
  display: flex;
  justify-content: flex-end;
  padding: 0 8px;
  border-bottom: 1px solid var(--el-border-color-lighter);
}
.panel-expand-bar {
  width: 24px;
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: var(--el-text-color-secondary);
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 6px;
  user-select: none;
}
.panel-expand-bar.left {
  height: 100%;
  align-self: stretch;
}
.panel-expand-bar:hover {
  color: var(--el-color-primary);
  background: var(--el-fill-color-light);
}
.panel-block {
  margin-top: 16px;
}
.panel-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 8px;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}
.ai-info {
  margin-top: 8px;
  padding: 8px;
  background: var(--el-fill-color-light);
  border-radius: 6px;
  font-size: 11px;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 200px;
  overflow-y: auto;
}


/* 文件名（右栏信息）：长文件名换行不溢出 */
.file-name {
  word-break: break-all;
}

</style>
