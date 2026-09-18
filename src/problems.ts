/**
 * The problem list: what is wrong with the current keybindings and what the
 * user can do about each one.
 *
 * Every row is actionable. A conflict names both sides, so either can be
 * jumped to, rebound, or cleared without hunting through 25 sections for it.
 */
import { errorCount, problems, type BindingInfo, type Problem } from "./keys";

export type ProblemsHost = {
  /** The effective bindings, defaults included. */
  bindings: () => BindingInfo[];
  /** Effective binding text for a setting. */
  valueOf: (path: string) => string;
  /** `既定` / `設定済み` / `未保存` for a setting. */
  stateLabel: (path: string) => string;
  /** Scroll to and highlight the setting's row. */
  onFocus: (path: string) => void;
  /** Open key capture for the setting. */
  onCapture: (path: string) => Promise<void>;
  /** Write `""` so the setting stops claiming its chord. */
  onDisable: (path: string) => void;
};

const esc = (s: string) =>
  s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

const KIND_LABEL: Record<Problem["kind"], string> = {
  invalid: "構文エラー",
  conflict: "キー衝突",
  risky: "端末依存",
};

const ADVICE: Record<Problem["kind"], string> = {
  invalid: "herdr が受け付けない値です。別のキーに録音し直してください。",
  conflict:
    "同じキーに複数の動作が割り当たっているため、意図しない動作になります。どちらか一方を録音し直すか、無効にしてください。",
  risky:
    "外側の端末がこのキーを herdr まで届けない可能性があります。prefix+ を付けた形か ctrl+英字 / ファンクションキーが確実です。",
};

/** Count for the toolbar button. */
export function problemSummary(host: ProblemsHost): { total: number; errors: number } {
  const ps = problems(host.bindings());
  return { total: ps.length, errors: errorCount(ps) };
}

export function openProblems(host: ProblemsHost): void {
  const back = document.createElement("div");
  back.className = "capture-back";
  back.innerHTML = `<div class="problems" role="dialog" aria-modal="true" aria-label="キー設定の問題">
      <div class="prob-head">
        <span class="prob-title">キー設定の問題</span>
        <span class="prob-count" id="prob-count"></span>
        <span class="cap-spacer"></span>
        <button id="prob-close">閉じる (Esc)</button>
      </div>
      <div class="prob-body" id="prob-body"></div>
    </div>`;
  document.body.appendChild(back);

  const bodyEl = back.querySelector<HTMLElement>("#prob-body")!;
  const countEl = back.querySelector<HTMLElement>("#prob-count")!;

  const close = () => {
    window.removeEventListener("keydown", onKey, true);
    back.remove();
  };
  function onKey(ev: KeyboardEvent) {
    if (ev.key === "Escape") {
      ev.preventDefault();
      close();
    }
  }

  const draw = () => {
    const ps = problems(host.bindings());
    const errors = errorCount(ps);
    countEl.innerHTML = ps.length
      ? `${errors ? `<b class="err">エラー ${errors}</b>` : ""}${
          ps.length - errors ? `<b class="warn">警告 ${ps.length - errors}</b>` : ""
        }`
      : `<b class="ok">問題なし</b>`;

    if (!ps.length) {
      bodyEl.innerHTML = `<div class="prob-empty">キー設定に問題はありません。<br />
        構文エラー・重複したキー・端末が届けにくいキーが見つかるとここに並びます。</div>`;
      return;
    }

    bodyEl.innerHTML = ps
      .map((p) => {
        const rows = p.paths
          .map((path) => {
            const value = host.valueOf(path);
            return `<div class="prob-row">
                <span class="prob-path">${esc(path)}</span>
                <code class="prob-val">${esc(value || '""')}</code>
                <span class="prob-state">${esc(host.stateLabel(path))}</span>
                <span class="cap-spacer"></span>
                <button class="ghost" data-go="${esc(path)}">この設定へ移動</button>
                <button class="ghost rec" data-rec="${esc(path)}">キーを録音</button>
                <button class="ghost" data-off="${esc(path)}">無効にする</button>
              </div>`;
          })
          .join("");
        return `<div class="prob-item ${p.severity}">
            <div class="prob-line">
              <span class="prob-badge ${p.severity}">${KIND_LABEL[p.kind]}</span>
              <code class="prob-chord">${esc(p.chord || '""')}</code>
              ${p.scope === "navigate" ? `<span class="prob-scope">navigate モード内</span>` : ""}
              <span class="prob-detail">${esc(p.detail)}</span>
            </div>
            <div class="prob-advice">${esc(ADVICE[p.kind])}</div>
            ${rows}
          </div>`;
      })
      .join("");
  };

  bodyEl.onclick = async (ev) => {
    const btn = (ev.target as HTMLElement).closest<HTMLButtonElement>("button[data-go],button[data-rec],button[data-off]");
    if (!btn) return;
    if (btn.dataset.go) {
      close();
      host.onFocus(btn.dataset.go);
    } else if (btn.dataset.rec) {
      // Capture is itself modal, so step out of the way and come back.
      const path = btn.dataset.rec;
      close();
      await host.onCapture(path);
      openProblems(host);
    } else if (btn.dataset.off) {
      host.onDisable(btn.dataset.off);
      draw(); // the list shrinks as problems are resolved
    }
  };

  back.querySelector<HTMLButtonElement>("#prob-close")!.onclick = close;
  back.onclick = (ev) => {
    if (ev.target === back) close();
  };
  window.addEventListener("keydown", onKey, true);
  draw();
}
