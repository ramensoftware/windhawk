import { ConfigProvider } from 'antd';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import useSWR from 'swr';
import ReactMarkdownCustom from '@app/components/ReactMarkdownCustom';
import { fetchText } from '@app/utils/swrHelpers';

const ErrorMessage = styled.div`
  color: var(--whui-text-muted);
  font-style: italic;
`;

// A changelog as it is published: written in English whatever the UI language is,
// so it renders left to right regardless of the app's direction.
export function ChangelogMarkdown({ markdown }: { markdown: string }) {
  return (
    <ConfigProvider direction="ltr">
      <ReactMarkdownCustom markdown={markdown} direction="ltr" />
    </ConfigProvider>
  );
}

interface Props {
  modId: string;
  loadingNode: React.ReactElement;
  // How the fetched document becomes content, for a caller that presents it as
  // something other than the whole text (splitting it at a version, say). The
  // fetch and its loading and failure branches stay here either way.
  renderMarkdown?: (markdown: string) => React.ReactElement;
}

function ModDetailsChangelog({ modId, loadingNode, renderMarkdown }: Props) {
  const { t } = useTranslation();

  const url = `https://mods.windhawk.net/changelogs/${modId}.md`;

  const { data, error, isLoading } = useSWR(url, fetchText);

  if (error) {
    const githubUrl = `https://github.com/ramensoftware/windhawk-mods/blob/pages/changelogs/${modId}.md`;
    return (
      <ErrorMessage>
        <Trans
          t={t}
          i18nKey="modDetails.changelog.loadingFailed"
          components={[<a href={githubUrl}>GitHub</a>]}
        />
      </ErrorMessage>
    );
  }

  if (isLoading) {
    return loadingNode;
  }

  const markdown = data || '';
  return renderMarkdown ? (
    renderMarkdown(markdown)
  ) : (
    <ChangelogMarkdown markdown={markdown} />
  );
}

export default ModDetailsChangelog;
