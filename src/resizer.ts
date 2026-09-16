/**
 * Drag-to-resize for the section sidebar.
 *
 * Section names run up to `ui.sidebar.agents.rows_by_agent`, which does not
 * fit the old fixed width, so the first run measures the real text and sizes
 * the panel to it. After that the user's width is remembered.
 */

export const clampWidth = (px: number, min: number, max: number): number =>
  Math.round(Math.min(max, Math.max(min, px)));

const read = (key: string): number | null => {
  try {
    const v = window.localStorage.getItem(key);
    const n = v === null ? NaN : Number(v);
    return Number.isFinite(n) ? n : null;
  } catch {
    return null; // private mode / blocked storage: fall back to auto-fit
  }
};

const write = (key: string, px: number) => {
  try {
    window.localStorage.setItem(key, String(px));
  } catch {
    /* not worth surfacing */
  }
};

type Options = {
  panel: HTMLElement;
  handle: HTMLElement;
  min: number;
  max: number;
  storageKey: string;
  /** Selector for the label whose text decides the auto-fit width. */
  labelSelector: string;
  /** Selector for the trailing badge that must also fit. */
  trailingSelector: string;
};

export function installResizer(o: Options) {
  const apply = (px: number) => {
    const w = clampWidth(px, o.min, o.max);
    o.panel.style.width = `${w}px`;
    return w;
  };

  /** Widest label text, measured with the panel's own font. */
  const autoFit = (): number => {
    const labels = [...o.panel.querySelectorAll<HTMLElement>(o.labelSelector)];
    if (!labels.length) return o.panel.offsetWidth;
    const ctx = document.createElement("canvas").getContext("2d");
    if (!ctx) return o.panel.offsetWidth;
    ctx.font = getComputedStyle(labels[0]).font;
    const text = labels.reduce((w, l) => Math.max(w, ctx.measureText(l.textContent ?? "").width), 0);
    const badge = [...o.panel.querySelectorAll<HTMLElement>(o.trailingSelector)].reduce(
      (w, b) => Math.max(w, b.offsetWidth),
      0
    );
    // button padding (16) + label/badge gap (8) + panel padding (16) + scrollbar (12)
    return Math.ceil(text + badge + 52);
  };

  const stored = read(o.storageKey);
  apply(stored ?? autoFit());

  let startX = 0;
  let startW = 0;

  const onMove = (ev: PointerEvent) => apply(startW + (ev.clientX - startX));
  const onUp = (ev: PointerEvent) => {
    o.handle.releasePointerCapture(ev.pointerId);
    o.handle.removeEventListener("pointermove", onMove);
    o.handle.removeEventListener("pointerup", onUp);
    o.handle.classList.remove("dragging");
    document.body.classList.remove("resizing");
    write(o.storageKey, o.panel.offsetWidth);
  };

  o.handle.addEventListener("pointerdown", (ev: PointerEvent) => {
    startX = ev.clientX;
    startW = o.panel.offsetWidth;
    o.handle.setPointerCapture(ev.pointerId);
    o.handle.addEventListener("pointermove", onMove);
    o.handle.addEventListener("pointerup", onUp);
    o.handle.classList.add("dragging");
    document.body.classList.add("resizing");
    ev.preventDefault();
  });

  // Double-click snaps back to "just wide enough for every section name".
  o.handle.addEventListener("dblclick", () => write(o.storageKey, apply(autoFit())));

  // Keyboard resizing so the handle is not mouse-only.
  o.handle.addEventListener("keydown", (ev: KeyboardEvent) => {
    const step = ev.shiftKey ? 32 : 8;
    if (ev.key === "ArrowLeft") write(o.storageKey, apply(o.panel.offsetWidth - step));
    else if (ev.key === "ArrowRight") write(o.storageKey, apply(o.panel.offsetWidth + step));
    else return;
    ev.preventDefault();
  });

  /** Re-fit after the nav is re-rendered, but only while untouched by the user. */
  return {
    refit: () => {
      if (read(o.storageKey) === null) apply(autoFit());
    },
  };
}
