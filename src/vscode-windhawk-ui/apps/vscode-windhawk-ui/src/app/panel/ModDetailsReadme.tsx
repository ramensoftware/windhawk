import ReactMarkdown from 'react-markdown';
import rehypeSlug from 'rehype-slug';

interface Props {
  markdown: string;
}

function ModDetailsReadme({ markdown }: Props) {
  return <ReactMarkdown children={markdown} rehypePlugins={[rehypeSlug]} />;
}

export default ModDetailsReadme;
