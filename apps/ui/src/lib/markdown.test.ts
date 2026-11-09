import { describe, expect, it } from 'vitest';
import { renderMarkdown } from './markdown';

describe('renderMarkdown', () => {
  it('renders headings, paragraphs and lists', () => {
    const md = '# Changelog\n\nIntro line\nwrapped.\n\n## 0.1\n\n- one\n- two\n  continued\n\n### Sub\nAfter.';
    expect(renderMarkdown(md)).toBe(
      '<h1>Changelog</h1><p>Intro line wrapped.</p><h2>0.1</h2><ul><li>one</li><li>two continued</li></ul><h3>Sub</h3><p>After.</p>',
    );
  });

  it('renders inline code and bold', () => {
    expect(renderMarkdown('Use `.lrc` files, **gapless**.')).toBe('<p>Use <code>.lrc</code> files, <strong>gapless</strong>.</p>');
  });

  it('escapes HTML', () => {
    expect(renderMarkdown('- <b>&</b> `<i>`')).toBe('<ul><li>&lt;b&gt;&amp;&lt;/b&gt; <code>&lt;i&gt;</code></li></ul>');
  });
});
