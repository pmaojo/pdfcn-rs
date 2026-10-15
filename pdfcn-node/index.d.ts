// pdfcn — TypeScript declarations for the napi-rs native bindings.

/** Page size: preset names or custom width×height in mm. */
export type PageSize = "a4" | "letter" | `${number}x${number}`;

/** Page orientation. */
export type Orientation = "portrait" | "landscape";

/** Color theme. */
export type Theme = "light" | "dark";

/** Image compression format. */
export type ImageFormat = "auto" | "jpeg" | "lossless" | "raw";

/** Options passed to every render call. */
export interface PageOptions {
  /** Page size: "a4" | "letter" | "WxH" in mm. Default: "a4". */
  size?: PageSize;
  /** "portrait" | "landscape". Default: "portrait". */
  orientation?: Orientation;
  /** Page margin in mm. Default: 20. */
  marginMm?: number;
  /**
   * Template format: "haml" | "html" | "markdown" | "json" | "yaml" | "typst" | "latex".
   * Omitted, it is detected from the text; YAML and Typst need it set.
   */
  format?: "haml" | "html" | "markdown" | "json" | "yaml" | "typst" | "latex";
  /** "light" | "dark". Default: "light". */
  theme?: Theme;
  /**
   * Text repeated on every page header (forward-compat: printpdf 0.12.6
   * doesn't render this yet).
   */
  headerText?: string;
  /**
   * Text repeated on every page footer (forward-compat: printpdf 0.12.6
   * doesn't render this yet).
   */
  footerText?: string;
  /** Appends "Page X of Y" to the footer. Same current no-op. */
  showPageNumbers?: boolean;
  /** Suppresses header/footer on first page. */
  skipFirstPage?: boolean;
  /** JPEG quality 0.0–1.0. Default: 0.85. */
  imageQuality?: number;
  /** Size budget per embedded image, e.g. "300kb" or "2MB". */
  imageMaxSize?: string;
  /** Force every embedded image to greyscale. */
  imageGreyscale?: boolean;
  /** Image compression: "auto" | "jpeg" | "lossless" | "raw". */
  imageFormat?: ImageFormat;
  /** PDF document title metadata. */
  title?: string;
  /** PDF document author metadata. */
  author?: string;
  /** PDF document subject metadata. */
  subject?: string;
  /** PDF document keywords metadata. */
  keywords?: string[];
  /**
   * SVG side-channel: maps ids (from `%Vector(id="...")` placeholders)
   * to SVG source strings. Use parallel arrays since NAPI can't express
   * a map keyset.
   */
  svgIds?: string[];
  svgSources?: string[];
}

/**
 * Renders a .haml-style `template` against a JSON `data` context and
 * returns PDF bytes as a Buffer.
 *
 * @example
 * ```ts
 * import { renderPdf } from "pdfcn";
 *
 * const pdf = renderPdf(
 *   '%DocumentLayout(size="a4")\n  %p {{ greeting }}',
 *   JSON.stringify({ greeting: "hello from TypeScript" }),
 *   { size: "a4", theme: "light" }
 * );
 * // pdf is a Buffer — write to disk or stream as a response.
 * ```
 *
 * @param template - A .haml template string (indentation-based, HAML-like).
 * @param dataJson - JSON-stringified data context for `{{ interpolation }}`.
 * @param options - Optional page, theme, and image configuration.
 * @returns PDF bytes as a Node.js Buffer.
 */
export function renderPdf(
  template: string,
  dataJson: string,
  options?: PageOptions
): Buffer;