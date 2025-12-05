import { ConfigProvider } from 'antd';
import type { Components } from 'react-markdown';
import ReactMarkdownCustom from '../components/ReactMarkdownCustom';

interface Props {
  markdown: string;
}

function ModDetailsReadme({ markdown }: Props) {
  const customComponents: Components = {
    img: ({ node, src, alt, ...props }) => {
      let transformedSrc = src;

      // Transform certain image URLs to go through our image proxy. This
      // ensures that the original images are available even if they're removed
      // from the original hosting site. Also, Imgur is blocked in the UK, so
      // this makes Imgur images accessible there.
      if (src) {
        const shouldTransform =
          src.startsWith('https://i.imgur.com/') ||
          (src.startsWith('https://raw.githubusercontent.com/') &&
            !src.startsWith('https://raw.githubusercontent.com/ramensoftware/'));

        if (shouldTransform) {
          const path = src.slice('https://'.length);
          transformedSrc = `https://mods.windhawk.net/images/${path}`;
        }
      }

      return <img src={transformedSrc} alt={alt} {...props} />;
    }
  };

  return (
    <ConfigProvider direction="ltr">
      <ReactMarkdownCustom
        markdown={markdown}
        components={customComponents}
        direction="ltr"
      />
    </ConfigProvider>
  );
}

export default ModDetailsReadme;
