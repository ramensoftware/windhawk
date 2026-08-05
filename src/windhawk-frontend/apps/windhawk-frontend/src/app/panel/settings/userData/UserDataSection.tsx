import { type UserDataManifest } from '@app/webviewIPCMessages';
import { Button, Space } from 'antd';
import { useCallback, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { ExportModal } from './ExportModal';
import { ImportModal } from './ImportModal';
import { ImportSourceModal } from './ImportSourceModal';

type ImportData = {
  // Bumped on each inspect so the ImportModal remounts with a fresh transaction.
  id: number;
  manifest: UserDataManifest;
  archive: string;
};

interface Props {
  // Called when an import has overwritten options on disk so Settings can refresh them:
  // as soon as the app settings are applied, and again once the import ends.
  onImported?: () => void;
}

// The Export / Import entry point in Settings. Export opens its dialog directly;
// Import opens the source dialog, which asks where the archive comes from and has the
// host validate it, then hands over to the import dialog over the returned manifest.
export function UserDataSection({ onImported }: Props) {
  const { t } = useTranslation();

  const [exportOpen, setExportOpen] = useState(false);
  const [importSourceOpen, setImportSourceOpen] = useState(false);
  const [importData, setImportData] = useState<ImportData | null>(null);
  const inspectIdRef = useRef(0);

  // The source dialog is left to take itself down from here: it reports the handoff
  // as it starts closing, so dropping it now would cut its animation short. Its own
  // onClose clears the flag once it is gone.
  const handleInspected = useCallback(
    (manifest: UserDataManifest, archive: string) => {
      inspectIdRef.current += 1;
      setImportData({ id: inspectIdRef.current, manifest, archive });
    },
    []
  );

  return (
    <>
      <Space wrap>
        <Button
          data-testid="user-data-export"
          onClick={() => setExportOpen(true)}
        >
          {t('settings.userData.exportAction')}
        </Button>
        <Button
          data-testid="user-data-import"
          onClick={() => setImportSourceOpen(true)}
        >
          {t('settings.userData.importAction')}
        </Button>
      </Space>

      <ExportModal open={exportOpen} onClose={() => setExportOpen(false)} />

      {importSourceOpen && (
        <ImportSourceModal
          onClose={() => setImportSourceOpen(false)}
          onInspected={handleInspected}
        />
      )}

      {importData && (
        <ImportModal
          key={importData.id}
          manifest={importData.manifest}
          archive={importData.archive}
          onClose={() => setImportData(null)}
          onImported={onImported}
        />
      )}
    </>
  );
}
