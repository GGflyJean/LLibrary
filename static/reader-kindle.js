import '/static/foliate/view.js'
import { MOBI } from '/static/foliate/mobi.js'
import { unzlibSync } from '/static/foliate/vendor/fflate.js'

const CHUNK_SIZE = 512 * 1024
const CACHE_LIMIT = 48

class RemoteSlice {
  constructor(file, start, end) {
    this.file = file
    this.start = start
    this.end = end
  }
  get size() {
    return Math.max(0, this.end - this.start)
  }
  async arrayBuffer() {
    const data = await this.file.read(this.start, this.end)
    return data.buffer
  }
}

class RemoteFile {
  constructor(url, size) {
    this.url = url
    this.size = size
    this.cache = new Map()
    this.full = null
  }
  async chunk(index) {
    const start = index * CHUNK_SIZE
    const end = Math.min(this.size, start + CHUNK_SIZE)
    if (this.full) return this.full.subarray(start, end)
    if (this.cache.has(index)) {
      const value = this.cache.get(index)
      this.cache.delete(index)
      this.cache.set(index, value)
      return value
    }
    if (start >= end) return new Uint8Array(0)
    const response = await fetch(this.url, {
      headers: { Range: `bytes=${start}-${end - 1}` },
    })
    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`)
    let data = new Uint8Array(await response.arrayBuffer())
    if (response.status !== 206) {
      if (data.length >= this.size) this.full = data
      data = data.subarray(start, end)
    }
    this.cache.set(index, data)
    while (this.cache.size > CACHE_LIMIT) {
      this.cache.delete(this.cache.keys().next().value)
    }
    return data
  }
  async read(start, end) {
    const from = Math.max(0, Math.min(this.size, start))
    const to = Math.max(from, Math.min(this.size, end))
    const first = Math.floor(from / CHUNK_SIZE)
    const last = Math.floor((to - 1) / CHUNK_SIZE)
    const parts = []
    let total = 0
    for (let index = first; index <= last; index++) {
      const chunk = await this.chunk(index)
      const chunkStart = index * CHUNK_SIZE
      const partFrom = Math.max(0, from - chunkStart)
      const partTo = Math.min(chunk.length, to - chunkStart)
      if (partTo > partFrom) {
        parts.push(chunk.subarray(partFrom, partTo))
        total += partTo - partFrom
      }
    }
    if (parts.length === 1) return parts[0].slice()
    const result = new Uint8Array(total)
    let offset = 0
    for (const part of parts) {
      result.set(part, offset)
      offset += part.length
    }
    return result
  }
  slice(start = 0, end = this.size) {
    return new RemoteSlice(this, start, end)
  }
}

const root = document.getElementById('kindle-reader')
const view = document.getElementById('kindle-view')
const status = document.getElementById('kindle-status')
const tocPanel = document.getElementById('reader-toc')
const tocList = document.getElementById('reader-toc-list')
const percentInput = document.getElementById('reader-percent')
const zoomValue = document.getElementById('reader-zoom-value')

if (root && view) {
  const fileUrl = root.dataset.url
  const fileSize = Number(root.dataset.size) || 0
  const fileId = root.dataset.fileId || '0'
  const progressKey = 'llibrary:kindle:progress:' + fileId
  const zoomKey = 'llibrary:kindle:zoom'
  let zoom = Number(localStorage.getItem(zoomKey) || 100)
  let ready = false
  let navigating = false

  const setStatus = (text) => {
    if (!status) return
    status.textContent = text || ''
    status.hidden = !text
  }

  const loadWithStatus = async (action) => {
    navigating = true
    setStatus('正在加载…')
    try {
      await action()
    } catch (error) {
      console.warn(error)
      setStatus('加载失败：' + (error?.message || error))
      return
    }
    if (navigating) {
      navigating = false
      setStatus('')
    }
  }

  const applyZoom = () => {
    zoom = Math.max(60, Math.min(220, zoom))
    if (zoomValue) zoomValue.textContent = zoom + '%'
    localStorage.setItem(zoomKey, String(zoom))
    if (!ready) return
    try {
      if (typeof view.renderer?.setStyles === 'function')
        view.renderer.setStyles(`html { font-size: ${zoom}% !important; }`)
    } catch (error) {
      console.warn(error)
    }
  }

  const buildToc = (items, depth) => {
    const list = document.createElement('ul')
    if (depth > 0) list.className = 'reader-toc-sub'
    for (const item of items) {
      const li = document.createElement('li')
      const link = document.createElement('a')
      link.href = '#'
      link.textContent = item.label || item.href || ''
      link.addEventListener('click', async (event) => {
        event.preventDefault()
        if (tocPanel) tocPanel.hidden = true
        await loadWithStatus(() => view.goTo(item.href))
      })
      li.appendChild(link)
      if (item.subitems && item.subitems.length)
        li.appendChild(buildToc(item.subitems, depth + 1))
      list.appendChild(li)
    }
    return list
  }

  view.addEventListener('relocate', (event) => {
    const location = event.detail || {}
    if (typeof location.fraction === 'number') {
      localStorage.setItem(progressKey, String(location.fraction))
      if (percentInput)
        percentInput.value = String(Math.round(location.fraction * 100))
    }
    if (navigating) {
      navigating = false
      setStatus('')
    }
  })

  const main = async () => {
    await customElements.whenDefined('foliate-view')
    setStatus('正在读取电子书…')
    try {
      if (fileSize > 0) {
        const remote = new RemoteFile(fileUrl, fileSize)
        const book = await new MOBI({ unzlib: unzlibSync }).open(remote)
        await view.open(book)
      } else {
        await view.open(fileUrl)
      }
    } catch (error) {
      console.error(error)
      setStatus('无法预览此文件：' + (error?.message || error))
      return
    }
    setStatus('正在渲染首页…')

    const saved = Number(localStorage.getItem(progressKey))
    try {
      await view.init({ showTextStart: true })
    } catch (error) {
      console.warn(error)
    }
    if (Number.isFinite(saved) && saved > 0) {
      try {
        await view.goToFraction(Math.min(1, saved))
      } catch (error) {
        console.warn(error)
      }
    }

    ready = true
    setStatus('')
    applyZoom()

    if (tocList) {
      const toc = view.book?.toc || []
      tocList.replaceChildren()
      if (!toc.length) tocList.textContent = '此文件没有目录'
      else tocList.appendChild(buildToc(toc, 0))
    }
  }

  const tocToggle = document.getElementById('reader-toc-toggle')
  const tocClose = document.getElementById('reader-toc-close')
  if (tocToggle) tocToggle.addEventListener('click', () => { if (tocPanel) tocPanel.hidden = false })
  if (tocClose) tocClose.addEventListener('click', () => { if (tocPanel) tocPanel.hidden = true })

  const prev = document.getElementById('reader-prev')
  const next = document.getElementById('reader-next')
  if (prev) prev.addEventListener('click', () => loadWithStatus(() => view.prev()))
  if (next) next.addEventListener('click', () => loadWithStatus(() => view.next()))

  const jump = document.getElementById('reader-jump')
  if (jump) jump.addEventListener('click', async () => {
    if (!ready) return
    const percent = Math.max(0, Math.min(100, Number(percentInput?.value) || 0))
    await loadWithStatus(() => view.goToFraction(percent / 100))
  })

  const zoomOut = document.getElementById('reader-zoom-out')
  const zoomIn = document.getElementById('reader-zoom-in')
  if (zoomOut) zoomOut.addEventListener('click', () => { zoom -= 10; applyZoom() })
  if (zoomIn) zoomIn.addEventListener('click', () => { zoom += 10; applyZoom() })

  document.addEventListener('keydown', (event) => {
    const tag = event.target && event.target.tagName
    if (tag === 'INPUT' || tag === 'TEXTAREA') return
    if (event.key === 'ArrowLeft') view.goLeft()
    if (event.key === 'ArrowRight') view.goRight()
  })

  let cleaned = false
  window.__llibraryCleanup = () => {
    if (cleaned) return
    cleaned = true
    try { view.close() } catch (error) { console.warn(error) }
  }

  main()
}
