import usePersistedFlag from '@app/panel/shared/usePersistedFlag';
import { ConfigProvider, Switch } from 'antd';
import { lazy, Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

// Split off with its editor, as the source view is, for the same reason.
const SourceDiffView = lazy(() => import('./SourceDiffView'));

const ConfigurationWrapper = styled.div`
  margin-bottom: 20px;

  > span {
    vertical-align: middle;
  }

  > button {
    margin-inline-start: 10px;
  }
`;

const DiffWrapper = styled.div`
  direction: ltr;

  // A diff says what changed through the row tints alone, and forced colors
  // (Windows High Contrast) flattens every one of them to the page background,
  // leaving added and removed lines indistinguishable. There is no system color
  // pair that carries insert-vs-delete, so keep the themed palette here; both
  // themes already meet contrast on their own.
  @media (forced-colors: active) {
    forced-color-adjust: none;
  }
`;

const SPLIT_VIEW_STORAGE_KEY = 'sourceDiffSplitView';

interface Props {
  oldSource: string;
  newSource: string;
}

function ModDetailsSourceDiff({ oldSource, newSource }: Props) {
  const { t } = useTranslation();

  const [splitView, toggleSplitView] = usePersistedFlag(SPLIT_VIEW_STORAGE_KEY);

  return (
    <ConfigProvider direction="ltr">
      <ConfigurationWrapper>
        <span>{t('modDetails.changes.splitView')}</span>
        <Switch checked={splitView} onChange={toggleSplitView} />
      </ConfigurationWrapper>
      <DiffWrapper>
        <Suspense fallback={null}>
          <SourceDiffView oldSource={oldSource} newSource={newSource} split={splitView} />
        </Suspense>
      </DiffWrapper>
    </ConfigProvider>
  );
}

export default ModDetailsSourceDiff;
