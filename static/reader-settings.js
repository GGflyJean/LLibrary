(function () {
  const STORAGE_KEY = 'llibrary:dblclick-next';
  const panel = document.getElementById('reader-settings');
  const toggle = document.getElementById('reader-settings-toggle');
  const close = document.getElementById('reader-settings-close');
  const checkbox = document.getElementById('reader-dblclick-toggle');

  function enabled() {
    return localStorage.getItem(STORAGE_KEY) !== '0';
  }

  function apply() {
    if (checkbox) checkbox.checked = enabled();
  }

  function open() {
    if (!panel) return;
    apply();
    panel.hidden = false;
  }

  function closePanel() {
    if (panel) panel.hidden = true;
  }

  if (toggle) {
    toggle.addEventListener('click', function (event) {
      event.stopPropagation();
      if (panel && !panel.hidden) closePanel();
      else open();
    });
  }
  if (close) close.addEventListener('click', closePanel);
  if (checkbox) {
    checkbox.addEventListener('change', function () {
      localStorage.setItem(STORAGE_KEY, checkbox.checked ? '1' : '0');
    });
  }
  document.addEventListener('click', function (event) {
    if (!panel || panel.hidden) return;
    if (event.target instanceof Element && (panel.contains(event.target) || (toggle && toggle.contains(event.target)))) return;
    closePanel();
  });
  document.addEventListener('keydown', function (event) {
    if (event.key === 'Escape') closePanel();
  });

  apply();
  window.llibraryDoubleClickNextEnabled = enabled;
})();
