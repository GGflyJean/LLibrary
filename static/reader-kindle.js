import '/static/foliate/view.js'

const root = document.getElementById('kindle-reader')
const view = document.getElementById('kindle-view')
const status = document.getElementById('kindle-status')
const tocPanel = document.getElementById('reader-toc')
const tocList = document.getElementById('reader-toc-list')
const percentInput = document.getElementById('reader-percent')
const zoomValue = document.getElementById('reader-zoom-value')

if (root && view) {
  const fileUrl = root.dataset.url
  const fileId = root.dataset.fileId || '0'
  const progressKey = 'llibrary:kindle:progress:' + fileId
  const zoomKey = 'llibrary:kindle:zoom'
  let zoom = Number(localStorage.getItem(zoomKey) || 100)
  let ready = false

  const setStatus = (text) => {
    if (!status) return
    status.textContent = text || ''
    status.hidden = !text
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
        try {
          await view.goTo(item.href)
        } catch (error) {
          console.warn(error)
        }
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
  })

  const main = async () => {
    await customElements.whenDefined('foliate-view')
    setStatus('正在解析电子书…')
    try {
      await view.open(fileUrl)
    } catch (error) {
      console.error(error)
      setStatus('无法预览此文件：' + (error?.message || error))
      return
    }

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
  if (prev) prev.addEventListener('click', () => view.prev())
  if (next) next.addEventListener('click', () => view.next())

  const jump = document.getElementById('reader-jump')
  if (jump) jump.addEventListener('click', async () => {
    if (!ready) return
    const percent = Math.max(0, Math.min(100, Number(percentInput?.value) || 0))
    try {
      await view.goToFraction(percent / 100)
    } catch (error) {
      console.warn(error)
    }
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
