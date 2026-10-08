// Measures music.youtube.com as the browser lays it out, in short lines.
//
// Run it in the page (the browser's console, or a browser tool), then:
//   ytm.m('ytmusic-player-bar')        one element, or the first of a kind
//   ytm.m('ytmusic-chip-cloud-chip-renderer', 3)   the first three
//   ytm.tree('ytmusic-player-bar', 6)  everything visible inside, 6 levels
//                                      deep, wrappers that add nothing left out
//   ytm.page()                         the main parts of whatever page is open
//
// Each line reads: name  x,y  width x height  then only what is not a
// default: font size/weight/line height, text colour (#rrggbb, @alpha),
// bg (background), r (corner radius), pad, mar (margin), gap, border,
// op (opacity), and the first words of its text. Sizes are CSS pixels,
// which are the app's points. Measure with the window 1280 wide.
(() => {
  const round = (n) => Math.round(n * 10) / 10;
  const color = (c) => {
    const m = c.match(/rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)(?:,\s*([\d.]+))?\)/);
    if (!m) return c;
    const hex = (n) => Math.round(+n).toString(16).padStart(2, '0');
    let s = '#' + hex(m[1]) + hex(m[2]) + hex(m[3]);
    if (m[4] !== undefined && +m[4] !== 1) s += '@' + (+m[4]).toFixed(2);
    return s;
  };
  const transparent = (c) => /rgba\(0, 0, 0, 0\)|transparent/.test(c);
  const ownText = (el) =>
    [...el.childNodes]
      .filter((n) => n.nodeType === 3)
      .map((n) => n.textContent.trim())
      .join(' ')
      .trim();
  const name = (el) => {
    let s = el.tagName.toLowerCase();
    if (el.id) s += '#' + el.id;
    const cls = typeof el.className === 'string' ? el.className.split(' ').filter((c) => c && !c.startsWith('style-scope') && c !== 'style-scope') : [];
    if (cls.length) s += '.' + cls.slice(0, 2).join('.');
    return s;
  };
  const styles = (el, s, withText) => {
    const out = [];
    const text = ownText(el) || (withText ? (el.innerText || '').trim().split('\n')[0] : '');
    if (text || el.tagName === 'INPUT' || el.tagName === 'YT-FORMATTED-STRING') {
      out.push(`${s.fontSize}/${s.fontWeight}/${s.lineHeight}`);
      if (!/^Roboto/.test(s.fontFamily)) out.push('font:' + s.fontFamily.split(',')[0]);
      if (s.letterSpacing !== 'normal') out.push('ls:' + s.letterSpacing);
      out.push(color(s.color));
    }
    if (!transparent(s.backgroundColor)) out.push('bg:' + color(s.backgroundColor));
    if (s.backgroundImage !== 'none' && !s.backgroundImage.startsWith('url')) out.push('bgimg:' + s.backgroundImage.slice(0, 90));
    if (s.borderRadius !== '0px') out.push('r:' + s.borderRadius);
    if (s.padding !== '0px') out.push('pad:' + s.padding);
    if (s.margin !== '0px') out.push('mar:' + s.margin);
    if (s.gap !== 'normal' && s.gap !== '0px') out.push('gap:' + s.gap);
    for (const side of ['Top', 'Right', 'Bottom', 'Left']) {
      const w = s['border' + side + 'Width'];
      if (w !== '0px' && s['border' + side + 'Style'] !== 'none') out.push(`b${side[0]}:${w} ${color(s['border' + side + 'Color'])}`);
    }
    if (s.opacity !== '1') out.push('op:' + s.opacity);
    if (s.boxShadow !== 'none') out.push('shadow:' + s.boxShadow.slice(0, 60));
    if (s.textTransform !== 'none') out.push('tt:' + s.textTransform);
    if (s.objectFit && s.objectFit !== 'fill') out.push('fit:' + s.objectFit);
    if (text) out.push('"' + text.slice(0, 32) + '"');
    return out;
  };
  // A YouTube element's attributes, which choose among the stylesheet's
  // variants (item-size, aspect-ratio, is-... flags).
  const attrs = (el) => {
    if (!el.tagName.startsWith('YTMUSIC-')) return '';
    const kept = [...el.attributes].filter((a) => !['class', 'style', 'id', 'role', 'tabindex', 'aria-hidden', 'hidden'].includes(a.name));
    return kept.length ? ' [' + kept.map((a) => (a.value ? `${a.name}=${a.value.slice(0, 24)}` : a.name)).join(' ') + ']' : '';
  };
  const line = (el, depth, withText) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return `${'  '.repeat(depth)}${name(el)}${attrs(el)}  ${round(r.x)},${round(r.y)} ${round(r.width)}x${round(r.height)}  ${styles(el, s, withText).join(' ')}`.trimEnd();
  };
  const pick = (target) => (typeof target === 'string' ? document.querySelector(target) : target);
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  };

  const m = (target, n = 1) => {
    const els = typeof target === 'string' ? [...document.querySelectorAll(target)].filter(visible).slice(0, n) : [target];
    if (!els.length) return `${target}: none visible`;
    return els.map((el) => line(el, 0, true)).join('\n');
  };

  const tree = (target, maxDepth = 8) => {
    const root = pick(target);
    if (!root) return `${target}: not found`;
    const lines = [];
    const walk = (el, depth, parentRect, shown) => {
      if (shown > maxDepth || !visible(el)) return;
      const tag = el.tagName;
      if (tag === 'path' || tag === 'g' || tag === 'circle' || tag === 'rect' || tag === 'SCRIPT' || tag === 'STYLE') return;
      const r = el.getBoundingClientRect();
      const s = getComputedStyle(el);
      const own = styles(el, s, false);
      const same = parentRect && Math.abs(r.x - parentRect.x) < 0.5 && Math.abs(r.y - parentRect.y) < 0.5 && Math.abs(r.width - parentRect.width) < 0.5 && Math.abs(r.height - parentRect.height) < 0.5;
      // A wrapper the same size as its parent, with nothing of its own, is left out.
      const show = el === root || !same || own.length > 0 || tag === 'svg' || tag === 'IMG';
      if (show) lines.push(line(el, shown, false));
      if (tag === 'svg') return;
      for (const child of el.children) walk(child, depth + 1, r, show ? shown + 1 : shown);
      if (el.shadowRoot) for (const child of el.shadowRoot.children) walk(child, depth + 1, r, show ? shown + 1 : shown);
    };
    walk(root, 0, null, 0);
    return lines.join('\n');
  };

  // The main parts of the open page, one line each (first of each kind).
  const page = () => {
    const kinds = [
      'ytmusic-nav-bar', 'ytmusic-search-box', '#mini-guide', 'ytmusic-guide-renderer', 'ytmusic-guide-entry-renderer',
      'ytmusic-browse-response #contents', 'ytmusic-section-list-renderer', 'ytmusic-chip-cloud-renderer', 'ytmusic-chip-cloud-chip-renderer',
      'ytmusic-carousel-shelf-renderer', 'ytmusic-carousel-shelf-basic-header-renderer', 'ytmusic-two-row-item-renderer',
      'ytmusic-responsive-list-item-renderer', 'ytmusic-shelf-renderer', 'ytmusic-grid-renderer', 'ytmusic-responsive-header-renderer',
      'ytmusic-immersive-header-renderer', 'ytmusic-detail-header-renderer', 'ytmusic-tabs', 'ytmusic-card-shelf-renderer',
      'ytmusic-player-bar', 'ytmusic-player-page', 'ytmusic-player-queue-item', 'tp-yt-paper-tab',
    ];
    return kinds.map((k) => m(k)).filter((l) => !l.endsWith('none visible')).join('\n');
  };

  window.ytm = { m, tree, page, color };
  return 'ytm ready: ytm.m(selector, n), ytm.tree(selector, depth), ytm.page()';
})();
