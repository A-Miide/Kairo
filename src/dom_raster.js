// KAIRO — a painted slide as a picture.
//
// The slide painter draws a look (gradients, stripes, grain, glow, text with its shadow) as ordinary page
// elements. For an export that must look like the thumbnail in another program (ProPresenter, PowerPoint),
// those elements are copied with every style written out, their pictures and fonts folded in as data, laid
// into an SVG and drawn on a canvas: the picture is what the app shows, whatever the other program can draw.
'use strict';

(function (root) {

  const blobToDataUrl = (blob) => new Promise((ok, bad) => {
    const r = new FileReader();
    r.onload = () => ok(r.result);
    r.onerror = () => bad(r.error);
    r.readAsDataURL(blob);
  });

  const dataCache = new Map();
  async function toDataUrl(url) {
    if (/^data:/.test(url)) return url;
    if (dataCache.has(url)) return dataCache.get(url);
    const p = fetch(url).then(r => { if (!r.ok) throw new Error('missing'); return r.blob(); }).then(blobToDataUrl);
    dataCache.set(url, p);
    try { return await p; } catch (e) { dataCache.delete(url); throw e; }
  }

  // url("…") parts of a CSS value, swapped for data. A picture that can't be read is dropped (none).
  async function inlineUrls(value, base) {
    const re = /url\((['"]?)(.*?)\1\)/g;
    const found = [...String(value).matchAll(re)];
    if (!found.length) return value;
    let out = value;
    for (const m of found) {
      let data = null;
      try { data = await toDataUrl(new URL(m[2], base || document.baseURI).href); } catch { /* left out */ }
      out = out.split(m[0]).join(data ? `url("${data}")` : 'none');
    }
    return out;
  }

  // The @font-face rules for the families a tree uses, their files folded in.
  async function fontCss(families) {
    const want = new Set([...families].map(f => f.replace(/^["']|["']$/g, '').toLowerCase()));
    const out = [];
    for (const sheet of Array.from(document.styleSheets)) {
      let rules;
      try { rules = Array.from(sheet.cssRules || []); } catch { continue; }
      for (const rule of rules) {
        if (rule.type !== 5) continue;   // CSSFontFaceRule
        const fam = (rule.style.getPropertyValue('font-family') || '').replace(/^["']|["']$/g, '').toLowerCase();
        if (!want.has(fam)) continue;
        const src = await inlineUrls(rule.style.getPropertyValue('src'), sheet.href || document.baseURI);
        out.push(`@font-face{font-family:${rule.style.getPropertyValue('font-family')};font-style:${rule.style.getPropertyValue('font-style') || 'normal'};font-weight:${rule.style.getPropertyValue('font-weight') || 'normal'};src:${src};${rule.style.getPropertyValue('unicode-range') ? 'unicode-range:' + rule.style.getPropertyValue('unicode-range') + ';' : ''}}`);
      }
    }
    return out.join('\n');
  }

  const URL_PROPS = new Set(['background-image', 'mask-image', '-webkit-mask-image', 'border-image-source', 'list-style-image', 'content']);

  // A deep copy of `el` with each element's computed style written onto it.
  async function styledCopy(el, families) {
    const copy = el.cloneNode(false);
    if (el.nodeType !== 1) return copy;
    const cs = getComputedStyle(el);
    let css = '';
    for (let i = 0; i < cs.length; i++) {
      const p = cs[i];
      let v = cs.getPropertyValue(p);
      if (URL_PROPS.has(p) && /url\(/.test(v)) v = await inlineUrls(v, document.baseURI);
      css += `${p}:${v};`;
    }
    copy.setAttribute('style', css);
    cs.fontFamily.split(',').forEach(f => families.add(f.trim()));
    if (el.tagName === 'IMG') {
      try { copy.setAttribute('src', await toDataUrl(new URL(el.currentSrc || el.src, document.baseURI).href)); } catch { /* left empty */ }
    } else if (el.tagName === 'CANVAS') {
      const img = document.createElement('img');
      img.setAttribute('src', el.toDataURL('image/png'));
      img.setAttribute('style', css);
      return img;
    } else if (el.tagName === 'VIDEO') {
      return document.createElement('div');   // a moving picture has no still here
    }
    for (const child of el.childNodes) {
      if (child.nodeType === 3) copy.appendChild(child.cloneNode());
      else if (child.nodeType === 1) copy.appendChild(await styledCopy(child, families));
    }
    return copy;
  }

  // `el` (laid out at w × h CSS pixels, attached to the page) as a full slide picture, a JPEG at `scale` times that size.
  async function elementToPicture(el, w, h, scale) {
    const families = new Set();
    const copy = await styledCopy(el, families);
    // Where the element sat on the page (off screen, say) is not part of the picture.
    ['left', 'top', 'right', 'bottom', 'inset-inline-start', 'inset-inline-end', 'inset-block-start', 'inset-block-end',
      'margin-left', 'margin-top', 'margin-right', 'margin-bottom', 'margin-inline-start', 'margin-inline-end', 'margin-block-start', 'margin-block-end',
      'translate', 'rotate', 'scale'].forEach(p => copy.style.removeProperty(p));
    copy.style.setProperty('position', 'relative');
    copy.style.setProperty('transform', 'none');
    copy.style.setProperty('z-index', 'auto');
    const fonts = await fontCss(families);
    // Safari's WebKit (the Mac app) writes the namespace itself; Chrome doesn't. Written twice, the picture is not
    // valid XML and WebKit refuses to draw it ("Slide 1 could not be drawn as a picture", owner, 18 Nov 2026).
    let xhtml = new XMLSerializer().serializeToString(copy);
    if (!/^<\w+[^>]*?\sxmlns="/.test(xhtml)) xhtml = xhtml.replace(/^<(\w+)/, '<$1 xmlns="http://www.w3.org/1999/xhtml"');
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}"><defs><style>${fonts}</style></defs><foreignObject x="0" y="0" width="${w}" height="${h}">${xhtml}</foreignObject></svg>`;
    const url = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg);
    const img = await new Promise((ok, bad) => { const i = new Image(); i.onload = () => ok(i); i.onerror = () => bad(new Error('The slide could not be drawn.')); i.src = url; });
    const canvas = document.createElement('canvas');
    canvas.width = Math.round(w * scale); canvas.height = Math.round(h * scale);
    const g = canvas.getContext('2d');
    // A slide is a full screen on black, as the output shows it. Over black it's opaque, so it's kept as a JPEG:
    // WebKit leaves the edge of a drawn page a little see-through, and as a PNG each slide was 2.9 MB (18 Nov 2026).
    g.fillStyle = '#000';
    g.fillRect(0, 0, canvas.width, canvas.height);
    g.drawImage(img, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise((ok, bad) => { try { canvas.toBlob(b => (b ? ok(b) : bad(new Error('The slide could not be saved as a picture.'))), 'image/jpeg', 0.92); } catch (e) { bad(e); } });
    return { bytes: new Uint8Array(await blob.arrayBuffer()), w: canvas.width, h: canvas.height, ext: 'jpeg' };
  }

  const API = { elementToPicture, toDataUrl };
  if (root) root.KairoDomRaster = API;
  if (typeof module !== 'undefined' && module.exports) module.exports = API;

})(typeof window !== 'undefined' ? window : null);
