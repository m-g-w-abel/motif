// shortcut: headings, paragraphs, `-` lists, `code` and **bold** only (what CHANGELOG.md uses); grow it if that does.
const escape = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const inline = (s: string) =>
  escape(s)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');

/** Renders a small subset of Markdown to HTML, escaping everything else. */
export function renderMarkdown(md: string): string {
  let html = '';
  let para: string[] = [];
  let items: string[] = [];
  const flush = () => {
    if (para.length) html += `<p>${inline(para.join(' '))}</p>`;
    if (items.length) html += `<ul>${items.map((item) => `<li>${inline(item)}</li>`).join('')}</ul>`;
    para = [];
    items = [];
  };
  for (const line of md.split('\n')) {
    const heading = /^(#{1,3}) (.*)/.exec(line);
    if (heading) {
      flush();
      html += `<h${heading[1].length}>${inline(heading[2])}</h${heading[1].length}>`;
    } else if (line.startsWith('- ')) {
      if (para.length) flush();
      items.push(line.slice(2));
    } else if (!line.trim()) flush();
    else if (items.length) items[items.length - 1] += ` ${line.trim()}`;
    else para.push(line.trim());
  }
  flush();
  return html;
}
