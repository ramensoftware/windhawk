import ReactMarkdown from 'react-markdown';
import rehypeSlug from 'rehype-slug';
import remarkGfm from 'remark-gfm';
import styled from 'styled-components';

const ReactMarkdownWithStyle = styled(ReactMarkdown)`
  // Word-wrap long lines.
  overflow-wrap: break-word;

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

  tr {
    background-color: var(--color-canvas-default);
    border-top: 1px solid var(--color-border-muted);
  }

  tr:nth-child(2n) {
    background-color: var(--color-canvas-subtle);
  }

  td,
  th {
    padding: 6px 13px;
    border: 1px solid var(--color-border-default);
  }

  th {
    font-weight: 600;
  }

  table img {
    background-color: transparent;
  }
`;

interface Props {
  markdown: string;
}

function ReactMarkdownCustom({ markdown }: Props) {
  return (
    <ReactMarkdownWithStyle
      children={markdown}
      rehypePlugins={[
        rehypeSlug,
        remarkGfm,
      ]}
    />
  );
}

export default ReactMarkdownCustom;
