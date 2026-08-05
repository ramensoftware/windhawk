import { Alert, Button } from 'antd';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

// Its own top margin rather than the search row's, so the gap under the heading
// does not change depending on whether the bar is there. The bottom margin
// collapses with that search row's 12px into the 20px the section puts between
// its other rows.
const Bar = styled(Alert)`
  margin-top: 12px;
  margin-bottom: 20px;
`;

interface Props {
  // How many installed mods have an update waiting. Renders nothing at zero, so
  // the caller can hand over the count without guarding the bar itself.
  count: number;
  onOpen: () => void;
}

/**
 * The line that announces the updates, above the installed mods.
 *
 * Presentational: it counts and it offers, and acts on nothing by itself.
 */
export function UpdatesAvailableBar({ count, onOpen }: Props) {
  const { t } = useTranslation();

  if (count <= 0) {
    return null;
  }

  return (
    <Bar
      type="info"
      showIcon
      data-testid="mod-updates-bar"
      message={t('modUpdates.bar.message', { count })}
      action={
        <Button
          size="small"
          type="primary"
          data-testid="mod-updates-open"
          onClick={onOpen}
        >
          {t('modUpdates.bar.openButton')}
        </Button>
      }
    />
  );
}

export default UpdatesAvailableBar;
