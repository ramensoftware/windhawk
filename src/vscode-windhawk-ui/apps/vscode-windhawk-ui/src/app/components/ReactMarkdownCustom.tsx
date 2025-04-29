import ReactMarkdown from 'react-markdown';
import rehypeSlug from 'rehype-slug';
import remarkGfm from 'remark-gfm';
import styled from 'styled-components';

const ReactMarkdownStyleWrapper = styled.div`
  // Word-wrap long lines.
  overflow-wrap: break-word;

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
}

function ReactMarkdownCustom({ markdown }: Props) {
  return (
    <ReactMarkdownStyleWrapper>
      <ReactMarkdown
        children={markdown}
        rehypePlugins={[
          rehypeSlug,
          remarkGfm,
        ]}
      />
    </ReactMarkdownStyleWrapper>
  );
}

export default ReactMarkdownCustom;
