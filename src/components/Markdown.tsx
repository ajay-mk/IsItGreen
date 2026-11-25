import type { Element } from 'hast';
import type { Blockquote, Paragraph, Root, RootContent } from 'mdast';
import { useEffect, useState } from 'react';
import ReactMarkdown, { type Components } from 'react-markdown';
import rehypeHighlight from 'rehype-highlight';
import rehypeRaw from 'rehype-raw';
import rehypeSanitize, { defaultSchema, type Options as SanitizeSchema } from 'rehype-sanitize';
import remarkGfm from 'remark-gfm';
import { fetchImage } from '../lib/transport';
import { isImageUrl, isSafeExternalUrl } from '../lib/url';
import type { ImageSource } from '../providers/types';
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

/**
 * GitHub's own rules for HTML in Markdown (what github.com lets through), plus
 * the classes and role our alerts add. Scripts, styles, event handlers,
 * comments and non-http(s) URLs are removed.
 */
const SCHEMA: SanitizeSchema = {
  ...defaultSchema,
  attributes: {
    ...defaultSchema.attributes,
    div: [...(defaultSchema.attributes?.div ?? []), ['className', 'md-alert', /^md-alert-[a-z]+$/], ['role', 'note']],
    p: [...(defaultSchema.attributes?.p ?? []), ['className', 'md-alert-title']],
  },
};

/** UUIDs name GitHub attachments, both in the link and in its signed form. */
const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i;

/**
 * Where to load an image from. GitHub's rendered description has the
 * fetchable form of each image: a signed link for attachments in private
 * repositories, or its proxy for other sites' images. Failing that, the
 * address itself, if it is on GitHub's image hosts.
 */
function imageSource(url: string, images: readonly ImageSource[]): string | null {
  const proxied = images.find((i) => i.canonical === url);
  if (proxied && isImageUrl(proxied.src)) return proxied.src;
  const id = UUID.exec(url)?.[0].toLowerCase();
  const signed = id ? images.find((i) => i.src.toLowerCase().includes(id)) : undefined;
  if (signed && isImageUrl(signed.src)) return signed.src;
  return isImageUrl(url) ? url : null;
}

/** Images already loaded, by source; few and kept for the session. */
const loaded = new Map<string, Promise<string>>();
const MAX_LOADED = 40;

function load(src: string): Promise<string> {
  let hit = loaded.get(src);
  if (!hit) {
    hit = fetchImage(src);
    hit.catch(() => loaded.delete(src));
    loaded.set(src, hit);
    // Drop the oldest, so a long session does not keep every image.
    if (loaded.size > MAX_LOADED) loaded.delete(loaded.keys().next().value!);
  }
  return hit;
}

/** An image, loaded through the backend; a link to it if that fails. */
function RemoteImage({ src, original, alt, width }: {
  src: string;
  original: string;
  alt: string;
  width: number | undefined;
}) {
  const [state, setState] = useState<{ src: string; data: string | null; failed: boolean }>({ src, data: null, failed: false });
  useEffect(() => {
    let live = true;
    load(src).then(
      (data) => live && setState({ src, data, failed: false }),
      () => live && setState({ src, data: null, failed: true }),
    );
    return () => {
      live = false;
    };
  }, [src]);
  const current = state.src === src ? state : { data: null, failed: false };
  const label = alt ? `Image: ${alt}` : 'Image';
  if (current.failed) {
    return isSafeExternalUrl(original) ? (
      <ExternalLink href={original} className="md-image">{label}</ExternalLink>
    ) : (
      <span className="md-image">{label}</span>
    );
  }
  if (!current.data) return <span className="md-image md-image-loading" role="img" aria-label={label} />;
  return <img className="md-img" src={current.data} alt={alt} width={width} />;
}

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
 * The text is untrusted. HTML in it goes through GitHub's own sanitising
 * rules, so `<details>` and `<img>` work while scripts, styles and event
 * handlers do not. Links only open where the rest of the app may (GitHub and
 * the connected GitLab); others show as text, with the address on hover.
 * Images are fetched by the backend from GitHub's image hosts only (the
 * content security policy blocks remote images); others become a link.
 */
export function Markdown({ source, base, images = [] }: {
  source: string;
  /** The page the text belongs to, for relative links. */
  base: string;
  /** The provider's own sources for the description's images. */
  images?: readonly ImageSource[];
}) {
  const components: Components = {
    a: ({ href, children }) => {
      const url = resolve(href, base);
      if (url && isSafeExternalUrl(url)) return <ExternalLink href={url}>{children}</ExternalLink>;
      return <span className="md-link-off" title={href}>{children}</span>;
    },
    img: ({ src, alt, width }) => {
      const url = typeof src === 'string' ? resolve(src, base) : null;
      const text = alt ? `Image: ${alt}` : 'Image';
      const fetchable = url ? imageSource(url, images) : null;
      if (url && fetchable) {
        const w = Number(width);
        return <RemoteImage src={fetchable} original={url} alt={alt ?? ''} width={Number.isFinite(w) && w > 0 ? w : undefined} />;
      }
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
        rehypePlugins={[
          rehypeRaw,
          [rehypeSanitize, SCHEMA],
          // Only fenced blocks that name their language: guessing would colour prose and logs.
          [rehypeHighlight, { detect: false }],
        ]}
        components={components}
      >
        {source}
      </ReactMarkdown>
    </div>
  );
}
