import { useRef } from 'react';
import type { Components } from 'react-markdown';
import ReactMarkdown from 'react-markdown';
import rehypeSlug from 'rehype-slug';
import remarkGfm from 'remark-gfm';
import styled from 'styled-components';
import type { PluggableList } from 'unified';
import { sanitizeUrl } from '../utils';
import { findFragmentTarget, getFragmentId } from './markdownFragmentLinks';
/// #if EXTENSION
import rehypeRaw from 'rehype-raw';
import rehypeSanitize from 'rehype-sanitize';
/// #endif

const ReactMarkdownStyleWrapper = styled.div<{ $direction?: 'ltr' | 'rtl' }>`
  // Word-wrap long lines.
  overflow-wrap: break-word;

  ${props => props.$direction && `
    direction: ${props.$direction};
    text-align: ${props.$direction === 'rtl' ? 'right' : 'left'};
  `}

  // Inline code style.

  code {
    color: var(--whui-preformat);
  }

  pre {
    margin-top: 0.4em;
    margin-bottom: 0.4em;
    background-color: var(--whui-inline-code-bg);
    border-radius: 2px;
    padding: 4px 8px;
  }

  :not(pre) > code {
    white-space: break-spaces;
    background-color: var(--whui-inline-code-bg);
    border-radius: 2px;
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
  direction?: 'ltr' | 'rtl';
}

function ReactMarkdownCustom({ markdown, components, allowHtml = false, direction }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);

  // Custom link component that sanitizes URLs
  const defaultComponents: Components = {
    a: ({ node, href, children, ...props }) => {
      // A fragment href names a heading of this document rather than a place to
      // navigate to, and sanitizeUrl has no scheme to validate in one. The
      // extension and Tauri builds mount a hash router, which would read a
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
    /// #if EXTENSION
    // CRITICAL: rehype-raw MUST come before rehype-sanitize
    rehypePlugins.push(rehypeRaw, [rehypeSanitize, sanitizeSchema]);
    /// #else
    throw new Error('allowHtml is not supported in website mode');
    /// #endif
  }

  const remarkPlugins: PluggableList = [remarkGfm];

  return (
    <ReactMarkdownStyleWrapper ref={containerRef} $direction={direction}>
      <ReactMarkdown
        children={markdown}
        components={mergedComponents}
        rehypePlugins={rehypePlugins}
        remarkPlugins={remarkPlugins}
      />
    </ReactMarkdownStyleWrapper>
  );
}

export default ReactMarkdownCustom;
