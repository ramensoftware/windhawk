import { DividedRow, DividedRowMeta, RowList } from '@app/components/RowList';
import { showErrorMessage, showInfoMessage } from '@app/feedback';
import useModalClose from '@app/panel/shared/useModalClose';
import { getDisplayModId, isLocalModId, testIdProps } from '@app/utils';
import {
  useGetRepositoryModSourceData,
  useInstallMod,
  useUpdateModConfig,
} from '@app/webviewIPC';
import {
  formatSuppression,
  parseSuppression,
  type UpdateSuppression,
} from '@app/webviewIPCMessages';
import { Button, Empty, Modal, Space, Spin, Tag } from 'antd';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { VersionSelectorModal } from '../mod-details/VersionSelectorModal';
import { useInstalledModsState } from '../mods-browser/useInstalledMods';
import { modHasUpdateOnOffer } from '../shared/updateOffer';

const ListBox = styled.div`
  border: 1px solid var(--whui-border);
  border-radius: 6px;
  background: var(--whui-card-background-color);
  max-height: 60vh;
  overflow: auto;
`;

const RowActions = styled(Space)`
  flex-shrink: 0;
  margin-inline-start: 12px;
`;

type Row = {
  modId: string;
  name: string;
  installedVersion?: string;
  latestVersion: string | null;
  suppression: UpdateSuppression | null;
  updateOffered: boolean;
};

interface Props {
  onClose: () => void;
}

/**
 * Every mod installed from the repository, the version of it on the machine,
 * and the way to move it to another one: for when an update breaks a mod, or a
 * Windows update breaks the version installed.
 *
 * Moving a mod installs the picked version from the repository, which is then
 * the version that runs; Windhawk keeps one version of a mod on the machine.
 * A move to anything other than the latest version turns the latest down, as
 * "Disable updates for version" does, so neither the update offer nor the
 * autoUpdateMods setting undoes the move, while a newer version is still
 * offered. "Keep this version" turns down every version, and "Allow updates"
 * lifts either.
 *
 * One install at a time: the rows' actions wait while one runs, and the dialog
 * stays up until it has answered.
 */
export function ModVersionsModal({ onClose }: Props) {
  const { t } = useTranslation();
  const { open, close, afterClose } = useModalClose(onClose);

  const { installedMods, applyInstalledModDetails } = useInstalledModsState({
    readListingOnMount: true,
  });
  const { installMod } = useInstallMod();
  const { getRepositoryModSourceData } = useGetRepositoryModSourceData();
  const { updateModConfig } = useUpdateModConfig();

  const [pickerModId, setPickerModId] = useState<string | null>(null);
  const [busy, setBusy] = useState<{ modId: string; version?: string } | null>(
    null
  );

  const mountedRef = useRef(false);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const rows = useMemo((): Row[] | undefined => {
    if (!installedMods) {
      return undefined;
    }
    return Object.entries(installedMods)
      .filter(([modId]) => !isLocalModId(modId))
      .map(([modId, mod]) => ({
        modId,
        name: mod.metadata?.name || getDisplayModId(modId),
        installedVersion: mod.metadata?.version,
        latestVersion: mod.latestVersion,
        suppression: parseSuppression(mod.config?.updatesDisabledForVersion ?? ''),
        updateOffered: modHasUpdateOnOffer(modId, mod),
      }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [installedMods]);

  // The host echoes the write back as setNewModConfig, which is what moves the
  // row; the reply only says whether it landed.
  const writeSuppression = useCallback(
    async (modId: string, stored: string) => {
      const result = await updateModConfig({
        modId,
        config: { updatesDisabledForVersion: stored },
      });
      return result.status === 'reply' && result.data.succeeded;
    },
    [updateModConfig]
  );

  const changeVersion = useCallback(
    async (modId: string, version: string) => {
      setPickerModId(null);
      const mod = installedMods?.[modId];
      if (!mod || version === mod.metadata?.version) {
        return;
      }
      const name = mod.metadata?.name || getDisplayModId(modId);

      setBusy({ modId, version });
      try {
        // The picked version's source, and the latest version's name: the
        // version the move turns down, read now rather than off the cached one,
        // which the host drops on every install and which an update check that
        // is switched off never fills in.
        const [picked, latest] = await Promise.all([
          getRepositoryModSourceData({ modId, version }),
          getRepositoryModSourceData({ modId }),
        ]);
        if (picked.status !== 'reply' || latest.status !== 'reply') {
          return;
        }
        const source = picked.data.data.source;
        if (!source) {
          showErrorMessage(t('modVersions.sourceFailed', { name, version }));
          return;
        }

        const result = await installMod({ modId, modSource: source });
        if (result.status !== 'reply') {
          return;
        }
        const { installedModDetails, error } = result.data;
        if (!installedModDetails) {
          // A missing compiler has already raised its own prompt; anything else
          // went to the compiler output.
          showErrorMessage(t('modVersions.installFailed', { name, version }));
          return;
        }
        applyInstalledModDetails(modId, installedModDetails, !error);

        // After the install, which clears a version pin when it commits.
        const latestVersion = latest.data.data.metadata?.version;
        if (latestVersion && latestVersion !== version) {
          await writeSuppression(
            modId,
            formatSuppression({ kind: 'pinned', version: latestVersion })
          );
        }
        showInfoMessage(t('modVersions.installed', { name, version }));
      } finally {
        if (mountedRef.current) {
          setBusy(null);
        }
      }
    },
    [
      installedMods,
      getRepositoryModSourceData,
      installMod,
      applyInstalledModDetails,
      writeSuppression,
      t,
    ]
  );

  const statusTag = (row: Row) => {
    if (row.suppression?.kind === 'all') {
      return <Tag color="orange">{t('modVersions.status.kept')}</Tag>;
    }
    if (row.suppression?.kind === 'pinned') {
      return (
        <Tag color="gold">
          {t('modVersions.status.skipping', { version: row.suppression.version })}
        </Tag>
      );
    }
    if (row.updateOffered) {
      return <Tag color="blue">{t('modVersions.status.updateAvailable')}</Tag>;
    }
    return null;
  };

  const pickerMod = pickerModId ? installedMods?.[pickerModId] : undefined;

  return (
    <>
      <Modal
        open={open}
        afterClose={afterClose}
        title={t('modVersions.title')}
        onCancel={busy ? undefined : close}
        closable={!busy}
        mask={{ closable: false }}
        width={720}
        centered
        wrapProps={testIdProps('mod-versions-modal')}
        footer={[
          <Button key="close" disabled={!!busy} onClick={close}>
            {t('general.actions.close')}
          </Button>,
        ]}
      >
        {!rows ? (
          <Spin />
        ) : rows.length === 0 ? (
          <Empty description={t('modVersions.empty')} />
        ) : (
          <ListBox>
            <RowList
              divided
              items={rows}
              rowKey="modId"
              itemRender={(row) => {
                const rowBusy = busy?.modId === row.modId;
                return (
                  <DividedRow data-testid="mod-version-row" data-mod-id={row.modId}>
                    <DividedRowMeta
                      title={
                        <span title={getDisplayModId(row.modId)}>
                          {row.name} {statusTag(row)}
                        </span>
                      }
                      description={
                        rowBusy && busy?.version
                          ? t('modVersions.installing', { version: busy.version })
                          : [
                              row.installedVersion
                                ? t('modVersions.installedVersion', {
                                    version: row.installedVersion,
                                  })
                                : t('modVersions.installedVersionUnknown'),
                              row.latestVersion &&
                                row.latestVersion !== row.installedVersion &&
                                t('modVersions.latestVersion', {
                                  version: row.latestVersion,
                                }),
                            ]
                              .filter(Boolean)
                              .join(' · ')
                      }
                    />
                    <RowActions>
                      <Button
                        size="small"
                        loading={rowBusy}
                        disabled={!!busy}
                        data-testid="mod-version-change"
                        onClick={() => setPickerModId(row.modId)}
                      >
                        {t('modVersions.changeVersion')}
                      </Button>
                      {row.suppression ? (
                        <Button
                          size="small"
                          disabled={!!busy}
                          data-testid="mod-version-allow-updates"
                          onClick={() => void writeSuppression(row.modId, '')}
                        >
                          {t('modDetails.updates.allow')}
                        </Button>
                      ) : (
                        <Button
                          size="small"
                          disabled={!!busy}
                          data-testid="mod-version-keep"
                          onClick={() =>
                            void writeSuppression(
                              row.modId,
                              formatSuppression({ kind: 'all' })
                            )
                          }
                        >
                          {t('modVersions.keepVersion')}
                        </Button>
                      )}
                    </RowActions>
                  </DividedRow>
                );
              }}
            />
          </ListBox>
        )}
      </Modal>
      {pickerModId && (
        <VersionSelectorModal
          modId={pickerModId}
          open={!!pickerModId}
          selectedVersion={pickerMod?.metadata?.version ?? null}
          onSelect={(version) => void changeVersion(pickerModId, version)}
          onCancel={() => setPickerModId(null)}
        />
      )}
    </>
  );
}

export default ModVersionsModal;
