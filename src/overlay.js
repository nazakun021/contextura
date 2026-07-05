const invoke = typeof window !== 'undefined' ? window.__TAURI__.core.invoke : null;
const listen = typeof window !== 'undefined' ? window.__TAURI__.event.listen : null;

if (typeof document !== 'undefined') {
  document.addEventListener('DOMContentLoaded', () => {
  const container = document.getElementById('overlay-container');
  const spinner = document.getElementById('status-spinner');
  const errorBanner = document.getElementById('error-banner');
  const errorTitle = document.getElementById('error-title');
  const errorDetail = document.getElementById('error-detail');
  let errorTimeout = null;
  let spinnerTimeout = null;

  listen('translation-update', (event) => {
    const payload = event.payload;
    // Hide spinner and error banner
    spinner.classList.add('hidden');
    errorBanner.classList.add('hidden');
    clearTimeout(spinnerTimeout);

    requestAnimationFrame(() => {
      // Fade out existing boxes before replacing
      const existing = container.querySelectorAll('.translation-box, .skeleton-box');
      existing.forEach((el) => el.classList.add('fade-out'));

      // After fade-out completes (150ms), clear and render collision-resolved boxes
      setTimeout(() => {
        container.innerHTML = '';

        // Apply collision avoidance to all boxes before rendering
        const resolved = resolveCollisions(
          payload.boxes.map((b) => ({
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
            _data: b,
          }))
        );

        for (const resolved_box of resolved) {
          const box = resolved_box._data;
          const div = document.createElement('div');
          div.className = 'translation-box fade-in';
          if (box.is_vertical) {
            div.classList.add('vertical');
          }

          div.style.left = `${box.x}px`;
          div.style.top = `${resolved_box.adjustedY}px`;

          // Use min dimensions so the box can grow if text is longer
          div.style.minWidth = `${box.width}px`;
          div.style.minHeight = `${box.height}px`;

          div.style.backgroundColor = box.bg_color;
          div.style.color = box.fg_color;

          div.textContent = box.translated;
          div.title = box.original; // Tooltip shows original

          container.appendChild(div);

          // Trigger fade-in on next frame
          requestAnimationFrame(() => div.classList.add('visible'));
        }
      }, 150);
    });
  });

  listen('translation-clear', () => {
    const existing = container.querySelectorAll('.translation-box, .skeleton-box');
    existing.forEach((el) => el.classList.add('fade-out'));
    setTimeout(() => { container.innerHTML = ''; }, 150);
    spinner.classList.add('hidden');
    errorBanner.classList.add('hidden');
  });

  listen('translation-started', (event) => {
    spinner.classList.remove('hidden');
    errorBanner.classList.add('hidden');
    clearTimeout(spinnerTimeout);
    spinnerTimeout = setTimeout(() => {
      spinner.classList.add('hidden');
    }, 10000);

    // Render skeleton loaders in place of existing boxes
    const payload = event?.payload;
    if (payload && Array.isArray(payload.boxes) && payload.boxes.length > 0) {
      container.innerHTML = '';
      const resolved = resolveCollisions(
        payload.boxes.map((b) => ({ x: b.x, y: b.y, width: b.width, height: b.height, _data: b }))
      );
      for (const resolved_box of resolved) {
        const box = resolved_box._data;
        const skel = document.createElement('div');
        skel.className = 'skeleton-box fade-in';
        skel.style.left = `${box.x}px`;
        skel.style.top = `${resolved_box.adjustedY}px`;
        skel.style.width = `${box.width}px`;
        skel.style.height = `${box.height}px`;
        container.appendChild(skel);
        requestAnimationFrame(() => skel.classList.add('visible'));
      }
    }
  });

  listen('translation-error', (event) => {
    const payload = event.payload;
    const title = payload?.title || 'Contextura Notice';
    const message = payload?.message || 'Translation engine restarted.';
    const detail = payload?.detail || '';
    const level = payload?.level || 'warning';

    errorTitle.textContent = title;
    errorDetail.textContent = detail ? `${message} ${detail}` : message;
    errorBanner.dataset.level = level;
    errorBanner.classList.remove('hidden');
  });

  const retryBtn = document.getElementById('error-retry');
  retryBtn.addEventListener('click', async () => {
    retryBtn.disabled = true;
    retryBtn.textContent = 'Retrying...';
    try {
      await invoke('reload_runtime');
    } catch (e) {
      console.error(e);
    } finally {
      retryBtn.disabled = false;
      retryBtn.textContent = 'Retry Connection';
    }
  });
});
}

/**
 * Resolves overlapping translation boxes by shifting them downward.
 *
 * Each box is processed in order. For each box, we check if its adjusted
 * position overlaps any previously settled box (horizontally AND vertically).
 * If so, we push it downward so it clears the colliding box, then re-check
 * against all settled boxes again. A 6px padding is applied between boxes.
 *
 * @param {Array<{x: number, y: number, width: number, height: number}>} boxes
 * @returns {Array<{x, y, width, height, adjustedY: number}>}
 */
function resolveCollisions(boxes) {
  const PADDING = 6;
  const settled = [];

  return boxes.map((box) => {
    let adjustedY = box.y;
    let changed = true;

    while (changed) {
      changed = false;
      for (const other of settled) {
        // Check horizontal overlap
        const hOverlap =
          box.x < other.x + other.width && box.x + box.width > other.x;
        // Check vertical overlap using adjustedY
        const vOverlap =
          adjustedY < other.adjustedY + other.height &&
          adjustedY + box.height > other.adjustedY;

        if (hOverlap && vOverlap) {
          // Shift this box below the colliding settled box
          adjustedY = other.adjustedY + other.height + PADDING;
          changed = true;
          break; // Re-check all settled from top
        }
      }
    }

    const result = { ...box, adjustedY };
    settled.push(result);
    return result;
  });
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = { resolveCollisions };
}

