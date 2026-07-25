import { useGetInstalledMods, useExportUserData } from '@app/webviewIPC';
import { type UserDataExportSummary } from '@app/webviewIPCMessages';
import { getDisplayModId, testIdProps } from '@app/utils';
import { Alert, Button, List, Modal, Result, Switch } from 'antd';
import { type CSSProperties, useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

import {
  buildSelection,
  exportRowsFromInstalledMods,
  initialExportState,
  isSelectionEmpty,
  type UserDataModRow,
  type UserDataSelectionState,
} from './selection';
import { UserDataSelectionForm } from './UserDataSelectionForm';

// Fills the fixed-height dialog body so the mod list inside the form is the only
// scroll region; the description and offline toggle stay pinned.
const Body = styled.div`
  display: flex;
  flex-direction: column;
  gap: 16px;
  flex: 1 1 auto;
  min-height: 0;
`;

const OfflineRow = styled.div`
  display: flex;
  align-items: flex-start;
  gap: 12px;
`;

const OfflineText = styled.div`
  flex: 1 1 auto;
`;

const OfflineTitle = styled.div`
  font-weight: 600;
`;

const OfflineDescription = styled.div`
  color: var(--whui-text-muted);
  font-size: 13px;
`;

// A floor for the form-phase body so the mod list stays usable on short windows. Below
// this the min-height wins over the 70vh cap: the modal grows past the viewport and the
// whole modal scrolls (the antd wrap), the list scrolling within it - a double scroll,
// but the two are far apart (list vs. whole modal) and only on small windows.
const FORM_BODY_MIN_HEIGHT = 600;

type Phase = 'form' | 'done';

interface Props {
  open: boolean;
  onClose: () => void;
}

export function ExportModal({ open, onClose }: Props) {
  const { t } = useTranslation();

  const [rows, setRows] = useState<UserDataModRow[]>([]);
  const [state, setState] = useState<UserDataSelectionState>({
    appSettings: true,
    perMod: {},
  });
  const [offline, setOffline] = useState(false);
  const [phase, setPhase] = useState<Phase>('form');
  const [summary, setSummary] = useState<UserDataExportSummary | null>(null);
  const [loaded, setLoaded] = useState(false);

  const { getInstalledMods } = useGetInstalledMods(
    useCallback((data) => {
      const built = exportRowsFromInstalledMods(data.installedMods);
      setRows(built);
      setState(initialExportState(built));
      setLoaded(true);
    }, [])
  );

  const { exportUserData, exportUserDataPending } = useExportUserData(
    useCallback((data) => {
      // A dismissed Save dialog is a benign no-op; a failure is auto-surfaced by the
      // IPC layer. Either way, stay on the form so the user can retry.
      if (data.canceled || !data.succeeded) {
        return;
      }
      setSummary(data.summary ?? { warnings: [] });
      setPhase('done');
    }, [])
  );

  // Reset the form whenever the modal transitions to open (pure state, so it runs in
  // render, mirroring UpdateModal); the installed-set fetch is the effect below.
  const [wasOpen, setWasOpen] = useState(false);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) {
      setPhase('form');
      setSummary(null);
      setOffline(false);
      setLoaded(false);
      setRows([]);
    }
  }

  useEffect(() => {
    if (open) {
      getInstalledMods({});
    }
  }, [open, getInstalledMods]);

  const selectionEmpty = isSelectionEmpty(rows, state);

  // The form phase pins its chrome and lets only the mod list scroll (one scroll
  // region): a flex body capped at 70vh, no body scroll. On a window too short for the
  // min-height floor, the floor wins over the cap so the modal grows past the viewport
  // and the whole modal scrolls instead. The done phase sizes to content.
  const maxBodyHeight = CSS.supports('height: 100dvh') ? '70dvh' : '70vh';
  const bodyStyle: CSSProperties =
    phase === 'form'
      ? {
          display: 'flex',
          flexDirection: 'column',
          minHeight: FORM_BODY_MIN_HEIGHT,
          maxHeight: maxBodyHeight,
          overflow: 'hidden',
        }
      : { maxHeight: maxBodyHeight, overflow: 'auto' };

  const handleExport = () => {
    exportUserData({
      selection: buildSelection(rows, state),
      options: { offline },
    });
  };

  const footer =
    phase === 'done'
      ? [
          <Button
            key="done"
            type="primary"
            data-testid="export-done"
            onClick={onClose}
          >
            {t('settings.userData.export.doneButton')}
          </Button>,
        ]
      : [
          <Button key="cancel" data-testid="export-cancel" onClick={onClose}>
            {t('general.actions.cancel')}
          </Button>,
          <Button
            key="export"
            type="primary"
            loading={exportUserDataPending}
            disabled={!loaded || selectionEmpty}
            data-testid="export-confirm"
            onClick={handleExport}
          >
            {t('settings.userData.export.exportButton')}
          </Button>,
        ];

  return (
    <Modal
      open={open}
      title={t('settings.userData.export.title')}
      onCancel={onClose}
      maskClosable={false}
      width={620}
      centered
      bodyStyle={bodyStyle}
      wrapProps={testIdProps('export-modal')}
      footer={footer}
    >
      {phase === 'done' ? (
        <Result
          status="success"
          title={t('settings.userData.export.doneTitle')}
          subTitle={
            summary && summary.warnings.length > 0
              ? undefined
              : t('settings.userData.export.doneSubtitle')
          }
        >
          {summary && summary.warnings.length > 0 && (
            <Alert
              type="warning"
              showIcon
              message={t('settings.userData.export.warningsTitle')}
              description={
                <List
                  size="small"
                  dataSource={summary.warnings}
                  renderItem={(warning) => (
                    <List.Item>
                      <strong>{getDisplayModId(warning.modId)}</strong>: {warning.message}
                    </List.Item>
                  )}
                />
              }
            />
          )}
        </Result>
      ) : (
        <Body>
          <div>{t('settings.userData.export.description')}</div>
          <UserDataSelectionForm
            rows={rows}
            state={state}
            onChange={setState}
            appSettingsAvailable
            disabled={exportUserDataPending}
          />
          <OfflineRow>
            <OfflineText>
              <OfflineTitle>
                {t('settings.userData.export.offlineTitle')}
              </OfflineTitle>
              <OfflineDescription>
                {t('settings.userData.export.offlineDescription')}
              </OfflineDescription>
            </OfflineText>
            <Switch
              checked={offline}
              disabled={exportUserDataPending}
              onChange={setOffline}
            />
          </OfflineRow>
        </Body>
      )}
    </Modal>
  );
}
