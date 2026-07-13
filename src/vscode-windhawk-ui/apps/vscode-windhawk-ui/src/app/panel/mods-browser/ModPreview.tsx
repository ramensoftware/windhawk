import { Empty } from 'antd';
import { produce } from 'immer';
import { useCallback, useEffect, useLayoutEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';
import styled from 'styled-components';
import { showInfoMessage } from '@app/feedback';
import { useGetInstalledMods, useSetNewModConfig } from '@app/webviewIPC';
import { type ModConfig, type ModMetadata } from '@app/webviewIPCMessages';
import { ModDetails } from '../mod-details';

const CenteredContainer = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;
`;

const CenteredContent = styled.div`
  margin: auto;

  // Without this the centered content looks too low.
  padding-bottom: 10vh; /* Fallback for older browsers */
  padding-bottom: 10dvh;
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
  > | null>(null);

  const { getInstalledMods } = useGetInstalledMods(
    useCallback((data) => {
      setInstalledMods(data.installedMods);
    }, [])
  );

  useEffect(() => {
    getInstalledMods({});
  }, [getInstalledMods]);

  useSetNewModConfig(
    useCallback(
      (data) => {
        const { modId, config: newConfig } = data;
        setInstalledMods((prev) =>
          prev &&
          produce(prev, (draft) => {
            if (draft[modId]?.config) {
              draft[modId].config = {
                ...draft[modId].config,
                ...newConfig,
              };
            }
          })
        );
      },
      []
    )
  );

  const disabledAction = useCallback(() => {
    showInfoMessage(t('modPreview.actionUnavailable'), 1);
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
        goBack={disabledAction}
        extensionProps={{
          installedModDetails: installedMods[displayedModId],
          updateMod: disabledAction,
          forkModFromSource: disabledAction,
          compileMod: disabledAction,
          enableMod: disabledAction,
          editMod: disabledAction,
          forkMod: disabledAction,
          deleteMod: disabledAction,
          updateModRating: disabledAction,
        }}
      />
    </ContentWrapper>
  );
}

export default ModPreview;
