(function () {
  const STORAGE_KEY = 'llibrary:nav-position';
  const POSITIONS = ['top', 'bottom', 'left', 'right'];
  const body = document.body;
  const panel = document.getElementById('reader-settings');
  const toggle = document.getElementById('reader-settings-toggle');
  const close = document.getElementById('reader-settings-close');
  const choices = panel ? Array.from(panel.querySelectorAll('[data-nav]')) : [];

  function currentPosition() {
    const saved = localStorage.getItem(STORAGE_KEY);
    return POSITIONS.includes(saved) ? saved : 'bottom';
  }

  function apply(position) {
    POSITIONS.forEach(function (item) {
      body.classList.toggle('page-nav-' + item, item === position);
    });
    choices.forEach(function (button) {
      button.classList.toggle('active', button.dataset.nav === position);
    });
    localStorage.setItem(STORAGE_KEY, position);
  }

  function open() {
    if (!panel) return;
    panel.hidden = false;
    apply(currentPosition());
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
  choices.forEach(function (button) {
    button.addEventListener('click', function () {
      apply(button.dataset.nav);
      closePanel();
    });
  });
  document.addEventListener('click', function (event) {
    if (!panel || panel.hidden) return;
    if (event.target instanceof Element && (panel.contains(event.target) || (toggle && toggle.contains(event.target)))) return;
    closePanel();
  });
  document.addEventListener('keydown', function (event) {
    if (event.key === 'Escape') closePanel();
  });

  apply(currentPosition());
})();
