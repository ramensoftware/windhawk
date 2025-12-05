import { ConfigProvider } from 'antd';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import useSWR from 'swr';
import ReactMarkdownCustom from '../components/ReactMarkdownCustom';
import { fetchText } from '../swrHelpers';

const ErrorMessage = styled.div`
  color: rgba(255, 255, 255, 0.45);
  font-style: italic;
`;

interface Props {
  modId: string;
  loadingNode: React.ReactElement;
}

function ModDetailsChangelog({ modId, loadingNode }: Props) {
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

  return (
    <ConfigProvider direction="ltr">
      <ReactMarkdownCustom markdown={data || ''} direction="ltr" />
    </ConfigProvider>
  );
}

export default ModDetailsChangelog;
