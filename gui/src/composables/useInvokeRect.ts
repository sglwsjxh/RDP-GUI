// SPDX-License-Identifier: AGPL-3.0-only
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

const rect = ref<Rect>({ x: 0, y: 0, width: 0, height: 0 })
const scale = ref(1)
let observer: ResizeObserver | null = null
let el: HTMLElement | null = null
let mql: MediaQueryList | null = null

// 计算物理像素 rect 并上报 Rust
function report() {
  if (!el) return
  const r = el.getBoundingClientRect()
  rect.value = {
    x: Math.round(r.x * scale.value),
    y: Math.round(r.y * scale.value),
    width: Math.round(r.width * scale.value),
    height: Math.round(r.height * scale.value),
  }
  void invoke('report_viewer_rect', rect.value)
}

export function useViewerRect(viewerEl: HTMLElement) {
  el = viewerEl

  onMounted(() => {
    if (!el) return
    // 监听元素尺寸变化
    observer = new ResizeObserver(report)
    observer.observe(el)
    // 监听 DPI 变化（window.devicePixelRatio 变化，如跨屏拖动）
    mql = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`)
    mql.addEventListener('change', updateScale)
    // 初始上报一次
    report()
  })

  onUnmounted(() => {
    observer?.disconnect()
    observer = null
    mql?.removeEventListener('change', updateScale)
    mql = null
    el = null
  })

  function updateScale() {
    scale.value = window.devicePixelRatio || 1
    report()
  }

  // 保留手动更新接口
  const update = (target: HTMLElement) => {
    el = target
    report()
  }

  const getRect = computed(() => rect.value)

  return {
    update,
    getRect,
  }
}

// 旧接口兼容
export function useInvokeRect() {
  const r = ref<Rect>({ x: 0, y: 0, width: 0, height: 0 })
  const update = (target: HTMLElement) => {
    const b = target.getBoundingClientRect()
    const s = window.devicePixelRatio || 1
    r.value = {
      x: Math.round(b.x * s),
      y: Math.round(b.y * s),
      width: Math.round(b.width * s),
      height: Math.round(b.height * s),
    }
  }
  const getRect = computed(() => r.value)
  return { update, getRect }
}
