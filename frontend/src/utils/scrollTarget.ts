/**
 * 找出当前真正在滚动的容器。
 *
 * 为什么需要：页面里存在多层嵌套容器（.app-main / .library / .wall-container），
 * 实际滚动的那一层取决于各层的高度与 overflow 组合，硬编码某一个会取到
 * scrollTop 恒为 0 的元素（此前"记录不到滚动位置、返回时回顶"即因此）。
 * 判定方式：从候选元素中挑出**内容溢出**（scrollHeight > clientHeight）的那个。
 */
export function findScrollContainer(): HTMLElement | null {
  const candidates: HTMLElement[] = []
  const main = document.querySelector<HTMLElement>('.app-main')
  if (main) candidates.push(main)
  document
    .querySelectorAll<HTMLElement>('.library, .wall-container, .waterfall-measure-wrap, .app-main *')
    .forEach((el) => {
      if (el.scrollHeight > el.clientHeight + 4) candidates.push(el)
    })
  // 取"溢出最多"的那个作为真正的滚动容器
  let best: HTMLElement | null = null
  let bestDelta = 0
  for (const el of candidates) {
    const delta = el.scrollHeight - el.clientHeight
    if (delta > bestDelta) {
      bestDelta = delta
      best = el
    }
  }
  return best ?? main
}