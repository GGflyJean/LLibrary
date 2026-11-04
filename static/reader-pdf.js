(function () {
  const reader = document.getElementById('pdf-reader');
  if (!reader || !window.pdfjsLib) return;
  const url = reader.dataset.url;
  const status = document.getElementById('pdf-status');
  const wrap = document.getElementById('pdf-canvas-wrap');
  const pageInput = document.getElementById('pdf-page');
  const pagesLabel = document.getElementById('pdf-pages');
  const prev = document.getElementById('pdf-prev');
  const next = document.getElementById('pdf-next');
  const jump = document.getElementById('pdf-jump');
  const zoomOut = document.getElementById('pdf-zoom-out');
  const zoomIn = document.getElementById('pdf-zoom-in');
  const zoomValue = document.getElementById('pdf-zoom-value');
  const fit = document.getElementById('pdf-fit');
  const tocPanel = document.getElementById('reader-toc');
  const tocList = document.getElementById('reader-toc-list');

  pdfjsLib.GlobalWorkerOptions.workerSrc = '/static/pdfjs/pdf.worker.min.js';

  let pdfDoc = null;
  let currentPage = 1;
  let scale = 1;
  let fitWidth = true;
  let renderTask = null;

  const loadingTask = pdfjsLib.getDocument({
    url: url,
    cMapUrl: '/static/pdfjs/cmaps/',
    cMapPacked: true,
    standardFontDataUrl: '/static/pdfjs/standard_fonts/',
    disableAutoFetch: true,
    rangeChunkSize: 65536
  });

  loadingTask.promise
    .then(async function (doc) {
      pdfDoc = doc;
      pageInput.max = String(doc.numPages);
      pagesLabel.textContent = String(doc.numPages);
      status.style.display = 'none';
      await renderPage(1);
      await loadOutline();
    })
    .catch(function (error) {
      status.textContent = 'PDF 加载失败：' + (error && error.message ? error.message : error);
    });

  async function renderPage(number) {
    if (!pdfDoc) return;
    const target = Math.max(1, Math.min(pdfDoc.numPages, Number(number) || 1));
    if (renderTask) {
      try { renderTask.cancel(); } catch (error) { /* ignore */ }
      renderTask = null;
    }
    const page = await pdfDoc.getPage(target);
    const available = Math.max(200, reader.clientWidth - 32);
    const base = page.getViewport({ scale: 1 });
    const useScale = fitWidth ? available / base.width : scale;
    const viewport = page.getViewport({ scale: useScale });
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const canvas = document.createElement('canvas');
    canvas.className = 'pdf-canvas';
    canvas.width = Math.floor(viewport.width * dpr);
    canvas.height = Math.floor(viewport.height * dpr);
    canvas.style.width = viewport.width + 'px';
    canvas.style.height = viewport.height + 'px';
    wrap.replaceChildren(canvas);
    const context = canvas.getContext('2d');
    context.setTransform(dpr, 0, 0, dpr, 0, 0);
    const task = page.render({ canvasContext: context, viewport: viewport });
    renderTask = task;
    try {
      await task.promise;
    } catch (error) {
      if (error && error.name !== 'RenderingCancelledException') {
        console.error(error);
      }
    }
    renderTask = null;
    currentPage = target;
    pageInput.value = String(target);
    scale = useScale;
    zoomValue.textContent = Math.round(useScale * 100) + '%';
    reader.scrollTop = 0;
  }

  function goTo(number) {
    renderPage(number).catch(function (error) { console.error(error); });
  }

  prev.addEventListener('click', function () { goTo(currentPage - 1); });
  next.addEventListener('click', function () { goTo(currentPage + 1); });
  jump.addEventListener('click', function () { goTo(Number(pageInput.value) || 1); });
  pageInput.addEventListener('keydown', function (event) {
    if (event.key === 'Enter') goTo(Number(pageInput.value) || 1);
  });
  zoomOut.addEventListener('click', function () {
    fitWidth = false;
    scale = Math.max(0.4, scale / 1.2);
    goTo(currentPage);
  });
  zoomIn.addEventListener('click', function () {
    fitWidth = false;
    scale = Math.min(4, scale * 1.2);
    goTo(currentPage);
  });
  fit.addEventListener('click', function () {
    fitWidth = true;
    goTo(currentPage);
  });

  let resizeTimer = null;
  window.addEventListener('resize', function () {
    if (!fitWidth) return;
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(function () { goTo(currentPage); }, 200);
  });

  document.getElementById('reader-toc-toggle').addEventListener('click', function () {
    tocPanel.hidden = false;
  });
  document.getElementById('reader-toc-close').addEventListener('click', function () {
    tocPanel.hidden = true;
  });

  async function loadOutline() {
    try {
      const outline = await pdfDoc.getOutline();
      tocList.replaceChildren();
      if (!outline || !outline.length) {
        tocList.textContent = '此 PDF 没有目录';
        return;
      }
      tocList.appendChild(buildOutline(outline));
    } catch (error) {
      tocList.textContent = '目录加载失败';
    }
  }

  function buildOutline(items) {
    const list = document.createElement('ul');
    items.forEach(function (item) {
      const li = document.createElement('li');
      const link = document.createElement('a');
      link.href = '#';
      link.textContent = item.title || '未命名章节';
      link.addEventListener('click', async function (event) {
        event.preventDefault();
        const page = await destToPage(item.dest);
        goTo(page);
        tocPanel.hidden = true;
      });
      li.appendChild(link);
      if (item.items && item.items.length) {
        li.appendChild(buildOutline(item.items));
      }
      list.appendChild(li);
    });
    return list;
  }

  async function destToPage(dest) {
    if (!dest) return 1;
    try {
      const explicit = typeof dest === 'string' ? await pdfDoc.getDestination(dest) : dest;
      if (!explicit || !explicit[0]) return 1;
      const index = await pdfDoc.getPageIndex(explicit[0]);
      return index + 1;
    } catch (error) {
      return 1;
    }
  }
})();
