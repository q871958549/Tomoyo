// Pure-TypeScript, XSS-safe Markdown + Checkbox + #Tag + [[WikiLink]] renderer (0 dependencies).

function escHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function renderInline(raw: string): string {
  let out = '';
  let i = 0;
  while (i < raw.length) {
    if (raw[i] === '`') {
      const end = raw.indexOf('`', i + 1);
      if (end > i + 1) {
        out += `<code class="md-code">${escHtml(raw.slice(i + 1, end))}</code>`;
        i = end + 1;
        continue;
      }
    }
    if (raw.startsWith('[[', i)) {
      const end = raw.indexOf(']]', i + 2);
      if (end > i + 2) {
        const inner = raw.slice(i + 2, end);
        const [target, alias] = inner.split('|');
        const cleanTarget = (target || '').trim();
        const label = (alias || cleanTarget).trim();
        if (cleanTarget) {
          out += `<button type="button" class="md-wikilink" data-wikilink="${escHtml(cleanTarget)}">[[${escHtml(label)}]]</button>`;
          i = end + 2;
          continue;
        }
      }
    }
    if (raw.startsWith('**', i)) {
      const end = raw.indexOf('**', i + 2);
      if (end > i + 2) {
        out += `<strong>${escHtml(raw.slice(i + 2, end))}</strong>`;
        i = end + 2;
        continue;
      }
    }
    const nextSpec = findNextSpecial(raw, i);
    const chunk = raw.slice(i, nextSpec);
    out += formatPlainSegment(chunk);
    if (nextSpec === i && i < raw.length) {
      out += escHtml(raw[i]);
      i += 1;
    } else {
      i = nextSpec;
    }
  }
  return out;
}

function findNextSpecial(s: string, from: number): number {
  const candidates = [s.indexOf('`', from), s.indexOf('[[', from), s.indexOf('**', from)].filter((x) => x >= 0);
  if (candidates.length === 0) return s.length;
  return Math.min(...candidates);
}

function formatPlainSegment(text: string): string {
  const escaped = escHtml(text);
  return escaped
    .replace(/(https?:\/\/[^\s<]+)/g, '<a href="#" class="md-link" data-ext-url="$1">$1</a>')
    .replace(/(^|\s)(#[^\s#.,;:，。；：!?！？]{1,28})/g, '$1<button type="button" class="md-tag" data-tag="$2">$2</button>');
}

export function renderNoteMarkdown(md: string): string {
  const lines = md.replace(/\r\n/g, '\n').split('\n');
  const html: string[] = [];
  let inCode = false;
  let codeBuf: string[] = [];

  for (let idx = 0; idx < lines.length; idx += 1) {
    const line = lines[idx];
    const trimmed = line.trim();

    if (trimmed.startsWith('```')) {
      if (inCode) {
        html.push(`<pre class="md-pre"><code>${escHtml(codeBuf.join('\n'))}</code></pre>`);
        codeBuf = [];
        inCode = false;
      } else {
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      codeBuf.push(line);
      continue;
    }
    if (!trimmed) {
      html.push('<div class="md-gap"></div>');
      continue;
    }
    if (/^---+$/.test(trimmed)) {
      html.push('<hr class="md-hr" />');
      continue;
    }
    const todoMatch = line.match(/^\s*[-*]\s+\[([ xX])\]\s*(.*)$/);
    if (todoMatch) {
      const done = todoMatch[1].toLowerCase() === 'x';
      const body = renderInline(todoMatch[2]);
      html.push(
        `<label class="md-todo ${done ? 'is-done' : ''}" data-todo-line="${idx}">` +
          `<input type="checkbox" ${done ? 'checked' : ''} data-todo-line="${idx}" />` +
          `<span>${body || '&nbsp;'}</span></label>`
      );
      continue;
    }
    if (trimmed.startsWith('### ')) {
      html.push(`<h3 class="md-h3">${renderInline(trimmed.slice(4))}</h3>`);
      continue;
    }
    if (trimmed.startsWith('## ')) {
      html.push(`<h2 class="md-h2">${renderInline(trimmed.slice(3))}</h2>`);
      continue;
    }
    if (trimmed.startsWith('# ')) {
      html.push(`<h1 class="md-h1">${renderInline(trimmed.slice(2))}</h1>`);
      continue;
    }
    if (trimmed.startsWith('> ')) {
      html.push(`<blockquote class="md-quote">${renderInline(trimmed.slice(2))}</blockquote>`);
      continue;
    }
    const bulletMatch = line.match(/^\s*[-*]\s+(.*)$/);
    if (bulletMatch) {
      html.push(`<div class="md-li"><span class="md-dot">•</span><span>${renderInline(bulletMatch[1])}</span></div>`);
      continue;
    }
    html.push(`<p class="md-p">${renderInline(line)}</p>`);
  }

  if (inCode) {
    html.push(`<pre class="md-pre"><code>${escHtml(codeBuf.join('\n'))}</code></pre>`);
  }
  return html.join('');
}
