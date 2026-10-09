import { Fragment } from 'react';
import { sanitizeUrl } from '../utils';
import { splitTextLinks } from './textLinks';

/**
 * Plain text, drawn as text, with each web address in it a link. For text that
 * is not markup and must not be read as any: nothing in it becomes an element
 * but the addresses.
 */
function LinkifiedText({ text }: { text: string }) {
  return (
    <>
      {splitTextLinks(text).map((part, i) => {
        if (part.kind === 'text') {
          return <Fragment key={i}>{part.text}</Fragment>;
        }
        const href = sanitizeUrl(part.url);
        return href ? (
          <a key={i} href={href}>
            {part.url}
          </a>
        ) : (
          <Fragment key={i}>{part.url}</Fragment>
        );
      })}
    </>
  );
}

export default LinkifiedText;
