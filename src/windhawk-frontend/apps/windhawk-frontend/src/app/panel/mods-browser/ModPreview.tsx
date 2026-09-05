import { Empty } from 'antd';
import { useCallback, useEffect, useLayoutEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';
import styled from 'styled-components';
import { useGetInstalledMods, useReloadInstalledMods } from '@app/webviewIPC';
import { ModDetails } from '../mod-details';
import { useInstalledModsState } from './useInstalledMods';

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

  // The same state the browsers hold, and the same host messages followed: this
  // screen shows a mod being written, and a config saved from the editor reaches
  // it the way it reaches them - the listing included, applied at the mark it
  // was asked for.
  const { installedMods, applyInstalledModsListing, modWriteMark } =
    useInstalledModsState();

  const { getInstalledMods } = useGetInstalledMods();

  const refreshInstalledMods = useCallback(async () => {
    const at = modWriteMark();
    const result = await getInstalledMods({});
    if (result.status === 'reply') {
      applyInstalledModsListing(result.data.installedMods, at);
    }
  }, [getInstalledMods, modWriteMark, applyInstalledModsListing]);

  useEffect(() => {
    void refreshInstalledMods();
  }, [refreshInstalledMods]);

  // The mod can be built, configured or removed while this screen is open, and
  // the listing is what says which of those happened. The host asks for it to be
  // read again when it has reason to think the machine moved under the screen.
  useReloadInstalledMods(refreshInstalledMods);

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
      {/* The preview is the whole of this screen: there is nowhere for a way
          back to lead, and no actions, so the buttons, the version list and the
          rating are all left out rather than shown as things that cannot run.
          The tabs reach the host themselves and go on working. */}
      <ModDetails
        modId={displayedModId}
        extensionProps={{
          installedModDetails: installedMods[displayedModId],
          // The editor reloads this screen while the mod is being written, which
          // is not the reader asking to be taken back to its details: the tab
          // they were reading comes back with it.
          remembersActiveTab: true,
        }}
      />
    </ContentWrapper>
  );
}

export default ModPreview;
