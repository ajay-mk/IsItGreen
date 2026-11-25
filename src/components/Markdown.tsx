import type { Element } from 'hast';
import type { Blockquote, Paragraph, Root, RootContent } from 'mdast';
import ReactMarkdown, { type Components } from 'react-markdown';
import rehypeHighlight from 'rehype-highlight';
import remarkGfm from 'remark-gfm';
import { isSafeExternalUrl } from '../lib/url';
import { ExternalLink } from './ExternalLink';

const ALERTS = {
  note: 'Note',
  tip: 'Tip',
  important: 'Important',
  warning: 'Warning',
  caution: 'Caution',
} as const;
type Alert = keyof typeof ALERTS;

const ALERT_MARKER = /^\[!(note|tip|important|warning|caution)\][ \t]*(?:\r?\n|$)/i;

/** The alert a blockquote opens with (`> [!NOTE]`), with the marker removed. */
function takeAlert(quote: Blockquote): Alert | null {
  const first = quote.children[0];
  if (first?.type !== 'paragraph') return null;
  const text = first.children[0];
  if (text?.type !== 'text') return null;
  const match = ALERT_MARKER.exec(text.value);
  if (!match) return null;
  text.value = text.value.slice(match[0].length);
  if (text.value === '') first.children.shift();
  // The marker on a line of its own leaves the paragraph starting with a break.
  if (first.children[0]?.type === 'break') first.children.shift();
  if (first.children.length === 0) quote.children.shift();
  return match[1]!.toLowerCase() as Alert;
}

/**
 * GitHub's alerts: a blockquote starting with `[!NOTE]`, `[!TIP]`,
 * `[!IMPORTANT]`, `[!WARNING]` or `[!CAUTION]` becomes a titled callout.
 */
function remarkAlerts() {
  const walk = (nodes: RootContent[]) => {
    for (const node of nodes) {
      if (node.type === 'blockquote') {
        const alert = takeAlert(node);
        if (alert) {
          const title: Paragraph = {
            type: 'paragraph',
            data: { hProperties: { className: ['md-alert-title'] } },
            children: [{ type: 'text', value: ALERTS[alert] }],
          };
          node.children.unshift(title);
          node.data = { hName: 'div', hProperties: { className: ['md-alert', `md-alert-${alert}`], role: 'note' } };
        }
      }
      if ('children' in node) walk(node.children as RootContent[]);
    }
  };
  return (tree: Root) => walk(tree.children);
}

/** The language of a fenced code block (` ```rust `), from its `<code>` child. */
function languageOf(pre: Element | undefined): string | null {
  const code = pre?.children[0];
  if (code?.type !== 'element' || code.tagName !== 'code') return null;
  const classes = code.properties.className;
  const lang = Array.isArray(classes) ? classes.map(String).find((c) => c.startsWith('language-')) : undefined;
  return lang ? lang.slice('language-'.length) : null;
}

/** Languages that describe a diagram, which GitHub draws instead of showing. */
const DIAGRAMS = new Set(['mermaid', 'geojson', 'topojson', 'stl']);

/** `href` resolved against the page it appears on, so relative links work. */
function resolve(href: string | undefined, base: string): string | null {
  if (!href) return null;
  try {
    return new URL(href, base).toString();
  } catch {
    return null;
  }
}

/**
 * GitHub-flavoured Markdown from an API (a pull request or issue description),
 * rendered as React elements: tables, task lists, strikethrough, autolinks,
 * GitHub's alerts, and code blocks highlighted by their language.
 *
 * The text is untrusted, so raw HTML in it is dropped rather than rendered,
 * and links only open where the rest of the app may (GitHub and the connected
 * GitLab); others show as text, with the address on hover. Images are not
 * loaded (the app's content security policy blocks remote ones) and become a
 * link to the image instead.
 */
export function Markdown({ source, base }: {
  source: string;
  /** The page the text belongs to, for relative links. */
  base: string;
}) {
  const components: Components = {
    a: ({ href, children }) => {
      const url = resolve(href, base);
      if (url && isSafeExternalUrl(url)) return <ExternalLink href={url}>{children}</ExternalLink>;
      return <span className="md-link-off" title={href}>{children}</span>;
    },
    img: ({ src, alt }) => {
      const url = typeof src === 'string' ? resolve(src, base) : null;
      const text = alt ? `Image: ${alt}` : 'Image';
      return url && isSafeExternalUrl(url) ? (
        <ExternalLink href={url} className="md-image">{text}</ExternalLink>
      ) : (
        <span className="md-image">{text}</span>
      );
    },
    pre: ({ node, children }) => {
      const lang = languageOf(node);
      if (lang && DIAGRAMS.has(lang.toLowerCase())) {
        return (
          <figure className="md-diagram">
            <figcaption>
              {lang === 'mermaid' ? 'Mermaid diagram' : `${lang} diagram`}, shown as source.{' '}
              {isSafeExternalUrl(base) && <ExternalLink href={base}>View it drawn on the web</ExternalLink>}
            </figcaption>
            <pre>{children}</pre>
          </figure>
        );
      }
      return (
        <div className={lang ? 'md-code md-code-labelled' : 'md-code'}>
          {lang && <span className="md-code-lang">{lang}</span>}
          <pre>{children}</pre>
        </div>
      );
    },
    // Task-list checkboxes show their state; they are not something to tick here.
    input: ({ type, checked }) =>
      type === 'checkbox' ? <input type="checkbox" checked={!!checked} disabled readOnly /> : null,
  };
  return (
    <div className="markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkAlerts]}
        // Only fenced blocks that name their language: guessing would colour prose and logs.
        rehypePlugins={[[rehypeHighlight, { detect: false }]]}
        skipHtml
        components={components}
      >
        {source}
      </ReactMarkdown>
    </div>
  );
}
