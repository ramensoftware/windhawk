import ReactMarkdownCustom from '../components/ReactMarkdownCustom';

interface Props {
  markdown: string;
}

function ModDetailsReadme({ markdown }: Props) {
  return <ReactMarkdownCustom markdown={markdown} />;
}

export default ModDetailsReadme;
