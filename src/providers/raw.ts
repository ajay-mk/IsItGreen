import type { ImageSource } from './types';

/**
 * Helpers for reading untrusted API responses: each one returns a safe
 * default instead of trusting the shape of the data.
 */

export type Raw = Record<string, unknown>;

export function obj(v: unknown): Raw | null {
  return typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Raw) : null;
}
export function str(v: unknown): string | null {
  return typeof v === 'string' ? v : null;
}
export function num(v: unknown): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : 0;
}
export function arr(v: unknown): unknown[] {
  return Array.isArray(v) ? v : [];
}

/** Longest description kept for the details view. */
export const DETAILS_BODY_CHARS = 400;

/** Longest Markdown description kept for the repository pop-up. */
export const MARKDOWN_CHARS = 20_000;

/** A Markdown description as written, cut to `MARKDOWN_CHARS`. */
export function capMarkdown(v: unknown): string {
  const text = (str(v) ?? '').replace(/\r\n?/g, '\n').trim();
  return text.length > MARKDOWN_CHARS ? `${text.slice(0, MARKDOWN_CHARS - 1).trimEnd()}…` : text;
}

const IMG_TAG = /<img\b[^>]*>/gi;
const attr = (tag: string, name: string): string | null => {
  const match = new RegExp(`\\s${name}="([^"]*)"`, 'i').exec(tag);
  return match ? decodeEntities(match[1]!) : null;
};
function decodeEntities(s: string): string {
  return s.replace(/&(amp|quot|#39|lt|gt);/g, (_, e: string) => ({ amp: '&', quot: '"', '#39': "'", lt: '<', gt: '>' })[e]!);
}

/** Longest rendered description searched for images. */
const MAX_HTML_CHARS = 200_000;

/**
 * The images in a provider's rendered description, in order. Only their
 * addresses are read; the HTML itself is never shown.
 */
export function imageSources(v: unknown): ImageSource[] {
  const html = (str(v) ?? '').slice(0, MAX_HTML_CHARS);
  return [...html.matchAll(IMG_TAG)].flatMap(([tag]) => {
    const src = attr(tag, 'src');
    return src ? [{ src, canonical: attr(tag, 'data-canonical-src') }] : [];
  });
}

/** Collapses whitespace and cuts a description to `DETAILS_BODY_CHARS`. */
export function trimBody(v: unknown): string {
  const text = (str(v) ?? '').replace(/\s+/g, ' ').trim();
  return text.length > DETAILS_BODY_CHARS ? `${text.slice(0, DETAILS_BODY_CHARS - 1).trimEnd()}…` : text;
}
