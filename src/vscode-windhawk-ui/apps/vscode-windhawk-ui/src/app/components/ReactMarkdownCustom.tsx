import type { Components } from 'react-markdown';
import ReactMarkdown from 'react-markdown';
import rehypeRaw from 'rehype-raw';
import rehypeSanitize from 'rehype-sanitize';
import rehypeSlug from 'rehype-slug';
import remarkGfm from 'remark-gfm';
import styled from 'styled-components';
import type { PluggableList } from 'unified';
import { sanitizeUrl } from '../utils';

const ReactMarkdownStyleWrapper = styled.div<{ $direction?: 'ltr' | 'rtl' }>`
  // Word-wrap long lines.
  overflow-wrap: break-word;

  ${props => props.$direction && `
    direction: ${props.$direction};
    text-align: ${props.$direction === 'rtl' ? 'right' : 'left'};
  `}

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
    border: 1px solid #434343;
  }
`;

interface Props {
  markdown: string;
  components?: Components;
  allowHtml?: boolean;
  direction?: 'ltr' | 'rtl';
}

function ReactMarkdownCustom({ markdown, components, allowHtml = false, direction }: Props) {
  // Custom link component that sanitizes URLs
  const defaultComponents: Components = {
    a: ({ node, href, children, ...props }) => {
      const sanitizedHref = sanitizeUrl(href);
      return <a href={sanitizedHref} {...props}>{children}</a>;
    }
  };

  // Merge provided components with default components
  const mergedComponents = {
    ...defaultComponents,
    ...components
  };

  // Minimal schema: only allow basic formatting tags
  const sanitizeSchema = {
    tagNames: [
      // Headings
      'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      // Text formatting
      'p', 'br', 'strong', 'b', 'em', 'i',
      // Lists
      'ul', 'ol', 'li',
      // Blockquotes
      'blockquote',
      // Code
      'code', 'pre',
      // Links
      'a'
    ],
    attributes: {
      a: ['href'] // Only href for links, no other attributes
    },
    protocols: {
      href: ['http', 'https', 'mailto'] // Safe protocols only
    },
    // Explicitly strip dangerous elements
    strip: ['script', 'style', 'iframe', 'object', 'embed', 'img', 'video', 'audio']
  };

  // CRITICAL: rehype-raw MUST come before rehype-sanitize
  const rehypePlugins: PluggableList = allowHtml
    ? [rehypeSlug, rehypeRaw, [rehypeSanitize, sanitizeSchema]]
    : [rehypeSlug];
  const remarkPlugins: PluggableList = [remarkGfm];

  return (
    <ReactMarkdownStyleWrapper $direction={direction}>
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
