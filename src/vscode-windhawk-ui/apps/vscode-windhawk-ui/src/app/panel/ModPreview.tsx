import { Empty, message } from 'antd';
import { useCallback, useEffect, useLayoutEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';
import styled from 'styled-components';
import { useGetInstalledMods } from '../webviewIPC';
import { ModConfig, ModMetadata } from '../webviewIPCMessages';
import { mockModsBrowserLocalInitialMods } from './mockData';
import ModDetails from './ModDetails';

const CenteredContainer = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;
`;

const CenteredContent = styled.div`
  margin: auto;

  // Without this the centered content looks too low.
  padding-bottom: 10vh;
`;

type ModDetailsType = {
  metadata: ModMetadata | null;
  config: ModConfig | null;
  updateAvailable?: boolean;
  userRating?: number;
};

interface Props {
  ContentWrapper: React.ComponentType<
    React.ComponentPropsWithoutRef<'div'> & { $hidden?: boolean }
  >;
}

function ModPreview({ ContentWrapper }: Props) {
  const { t } = useTranslation();

  useLayoutEffect(() => {
    const header = document.querySelector('header');
    if (header) {
      header.style.display = 'none';
    }
  }, []);

  const { modId: displayedModId } = useParams<{
    modId: string;
  }>();

  const [installedMods, setInstalledMods] = useState<Record<
    string,
    ModDetailsType
  > | null>(mockModsBrowserLocalInitialMods);

  const { getInstalledMods } = useGetInstalledMods(
    useCallback((data) => {
      setInstalledMods(data.installedMods);
    }, [])
  );

  useEffect(() => {
    getInstalledMods({});
  }, [getInstalledMods]);

  const disabledAction = useCallback(() => {
    message.info(t('modPreview.actionUnavailable'), 1);
  }, [t]);

  if (!installedMods || !displayedModId) {
    return null;
  }

  if (!installedMods[displayedModId]) {
    return (
      <CenteredContainer>
        <CenteredContent>
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={t('modPreview.notCompiled')}
          />
        </CenteredContent>
      </CenteredContainer>
    );
  }

  return (
    <ContentWrapper>
      <ModDetails
        modId={displayedModId}
        installedModDetails={installedMods[displayedModId]}
        goBack={disabledAction}
        updateMod={disabledAction}
        forkModFromSource={disabledAction}
        compileMod={disabledAction}
        enableMod={disabledAction}
        editMod={disabledAction}
        forkMod={disabledAction}
        deleteMod={disabledAction}
        updateModRating={disabledAction}
      />
    </ContentWrapper>
  );
}

export default ModPreview;
