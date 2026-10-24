import { type ReactNode, useRef } from 'react';
import type { Components } from 'react-markdown';
import ReactMarkdown from 'react-markdown';
import rehypeSlug from 'rehype-slug';
import remarkBreaks from 'remark-breaks';
import remarkGfm from 'remark-gfm';
import styled from 'styled-components';
import type { PluggableList } from 'unified';
import { sanitizeUrl } from '../utils';
import { findFragmentTarget, getFragmentId } from './markdownFragmentLinks';
/// #if APP
import rehypeRaw from 'rehype-raw';
import rehypeSanitize from 'rehype-sanitize';
/// #endif

// As the dir attribute takes it; 'auto' follows the text's own script.
type Direction = 'ltr' | 'rtl' | 'auto';

const ReactMarkdownStyleWrapper = styled.div<{ $direction?: Direction }>`
  // Word-wrap long lines.
  overflow-wrap: break-word;

  // The wrapper's dir attribute sets the direction; the alignment follows it
  // rather than whatever an ancestor aligns to.
  ${props => props.$direction && `
    text-align: start;
  `}

  // A paragraph holding nothing but a dropped image would still take its
  // margin.
  p:empty {
    display: none;
  }

  // Inline code style.

  code {
    color: var(--whui-preformat);
  }

  pre {
    margin-top: 0.4em;
    margin-bottom: 0.4em;
    background-color: var(--whui-inline-code-bg);
    border-radius: var(--whui-radius);
    padding: 4px 8px;
  }

  :not(pre) > code {
    white-space: break-spaces;
    background-color: var(--whui-inline-code-bg);
    border-radius: var(--whui-radius-sm);
    padding: 1px 4px;
  }

  // Table style.
  // https://github.com/micromark/micromark-extension-gfm-table#css

  table {
    border-spacing: 0;
    border-collapse: collapse;
    display: block;
    margin-top: 0;
    margin-bottom: 16px;
    width: max-content;
    max-width: 100%;
    overflow: auto;
  }

  td,
  th {
    padding: 6px 13px;
    border: 1px solid var(--whui-border-strong);
  }
`;

interface Props {
  markdown: string;
  components?: Components;
  allowHtml?: boolean;
  // Off, an image is dropped, alt text and all.
  allowImages?: boolean;
  // A newline inside a paragraph as a line break rather than a soft wrap: how
  // text typed into a box reads, as against a document authored in markdown.
  breaks?: boolean;
  direction?: Direction;
  className?: string;
  // A say over how a link out of the document is drawn, given its sanitized
  // href: what it returns is drawn in place of the plain anchor, and undefined
  // leaves the anchor as it is. A link into the document is not offered.
  renderLink?: (href: string, children: ReactNode) => ReactNode | undefined;
}

function ReactMarkdownCustom({
  markdown,
  components,
  allowHtml = false,
  allowImages = true,
  breaks = false,
  direction,
  className,
  renderLink,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);

  // Custom link component that sanitizes URLs
  const defaultComponents: Components = {
    a: ({ node, href, children, ...props }) => {
      // A fragment href names a heading of this document rather than a place to
      // navigate to, and sanitizeUrl has no scheme to validate in one. The
      // app build mounts a hash router, which would read a
      // followed fragment as a route and leave the document, so the move is made
      // here instead of by the browser.
      const fragmentId = getFragmentId(href);
      if (fragmentId !== undefined) {
        return (
          <a
            {...props}
            href={`#${fragmentId}`}
            onClick={event => {
              event.preventDefault();
              const container = containerRef.current;
              if (container) {
                findFragmentTarget(container, fragmentId)?.scrollIntoView();
              }
            }}
          >
            {children}
          </a>
        );
      }

      const sanitizedHref = sanitizeUrl(href);
      if (sanitizedHref !== undefined && renderLink) {
        const rendered = renderLink(sanitizedHref, children);
        if (rendered !== undefined) {
          return rendered;
        }
      }
      return <a href={sanitizedHref} {...props}>{children}</a>;
    }
  };

  // Merge provided components with default components
  const mergedComponents = {
    ...defaultComponents,
    ...components
  };

  // Minimal schema: only allow basic formatting tags. An element named in
  // neither list below is replaced by its children, so everything the markdown
  // plugins can emit has to be named in one of the two to be either rendered or
  // dropped deliberately. Every key left out keeps its hast-util-sanitize
  // default (ancestor rules, id clobbering, comments, doctypes): the schema is
  // merged over the default one key deep.
  const sanitizeSchema = {
    tagNames: [
      // Headings
      'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      // Text formatting
      'p', 'br', 'strong', 'b', 'em', 'i', 'del',
      // Lists
      'ul', 'ol', 'li',
      // Blockquotes
      'blockquote',
      // Code
      'code', 'pre',
      // Tables
      'table', 'thead', 'tbody', 'tr', 'th', 'td',
      // Thematic breaks
      'hr',
      // Links
      'a'
    ],
    attributes: {
      a: ['href'], // Only href for links, no other attributes
      // rehype-slug puts the anchor ids on headings; without these the sanitizer
      // strips them back off and heading links stop resolving.
      h1: ['id'], h2: ['id'], h3: ['id'], h4: ['id'], h5: ['id'], h6: ['id'],
      // A table column's alignment reaches the document on each of its cells.
      th: [['align', 'left', 'center', 'right']],
      td: [['align', 'left', 'center', 'right']]
    },
    protocols: {
      href: ['http', 'https', 'mailto'] // Safe protocols only
    },
    // Deleted with their content rather than replaced by it, so that a script or
    // a stylesheet cannot reach the document as text. Remote media is refused
    // here as well as by the hosts' CSP, and a task list's checkbox is the one
    // form control markdown emits.
    strip: ['script', 'style', 'iframe', 'object', 'embed', 'img', 'video', 'audio', 'input']
  };

  const rehypePlugins: PluggableList = [rehypeSlug];
  if (allowHtml) {
    /// #if APP
    // CRITICAL: rehype-raw MUST come before rehype-sanitize
    rehypePlugins.push(rehypeRaw, [rehypeSanitize, sanitizeSchema]);
    /// #else
    throw new Error('allowHtml is not supported in website mode');
    /// #endif
  }

  const remarkPlugins: PluggableList = [remarkGfm];
  if (breaks) {
    remarkPlugins.push(remarkBreaks);
  }

  return (
    <ReactMarkdownStyleWrapper
      ref={containerRef}
      className={className}
      dir={direction}
      $direction={direction}
    >
      <ReactMarkdown
        children={markdown}
        components={mergedComponents}
        rehypePlugins={rehypePlugins}
        remarkPlugins={remarkPlugins}
        disallowedElements={allowImages ? undefined : ['img']}
      />
    </ReactMarkdownStyleWrapper>
  );
}

export default ReactMarkdownCustom;
