import { getDisplayModId } from '@app/utils';
import { faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Checkbox, Tag, Tooltip } from 'antd';
import { produce } from 'immer';
import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

import {
  type UserDataModRow,
  type UserDataSelectionState,
} from './selection';

// Fills the height its container (the dialog body) allots, so the mod list - not the
// dialog - is what scrolls when the list is long (a single scroll region).
const FormWrapper = styled.div`
  display: flex;
  flex-direction: column;
  gap: 16px;
  flex: 1 1 auto;
  min-height: 0;
`;

// The "Mods" label plus the list, growing to fill the form's leftover height so the
// list can be the scroller.
const ModsSection = styled.div`
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
`;

const SectionLabel = styled.div`
  font-weight: 600;
`;

const ModList = styled.div`
  border: 1px solid var(--whui-border);
  border-radius: 6px;
  background: var(--whui-card-background-color);
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
`;

const HeaderRow = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--whui-border);
  position: sticky;
  top: 0;
  background: var(--whui-card-background-color);
  z-index: 1;
`;

const ModRow = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 12px;

  &:not(:last-child) {
    border-bottom: 1px solid var(--whui-divider);
  }
`;

// The include checkbox grows to take the row's slack; the facet columns are fixed so
// they line up down the list.
const IncludeCell = styled.div`
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;

  /* Keep the row's fixed-width extras (the "local" tag) from being squeezed, so only
     the name gives up space. */
  .ant-tag {
    flex-shrink: 0;
  }

  /* Let a long mod name ellipsize within the include column instead of overflowing
     across the facet checkbox columns when the modal is narrow. antd's checkbox label
     is an inline-flex whose default min-width:auto refuses to shrink below its text, so
     opt it into shrinking and clip its label span. */
  .ant-checkbox-wrapper {
    min-width: 0;
  }

  .ant-checkbox-wrapper > span:last-child {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
`;

// Carries the hover title with the full mod id; the clipping is done by the enclosing
// checkbox label (see IncludeCell).
const ModName = styled.span``;

// Extra width around the widest facet label so its title has breathing room within the
// column.
const FACET_COLUMN_MARGIN = 16;

// The facet columns take their width from a CSS variable that is measured at runtime to
// fit the wider of the two titles (see the measurer below), so they adapt to whatever a
// translation makes the titles. The fallback covers the first render before measurement.
const FacetCell = styled.div`
  flex: 0 0 auto;
  width: var(--facet-column-width, 96px);
  display: flex;
  justify-content: center;
`;

const FacetLabel = styled.div`
  flex: 0 0 auto;
  width: var(--facet-column-width, 96px);
  text-align: center;
  color: var(--whui-text-muted);
  font-size: 13px;
  white-space: nowrap;
`;

// An off-screen twin of the facet titles, used only to measure their rendered width. It
// must carry the same font as FacetLabel so the measurement matches what is displayed.
const FacetMeasurer = styled.div`
  position: absolute;
  left: -9999px;
  top: 0;
  visibility: hidden;
  pointer-events: none;
  white-space: nowrap;
  font-size: 13px;

  > span {
    display: inline-block;
  }
`;

const EmptyNotice = styled.div`
  padding: 12px;
  color: var(--whui-text-muted);
`;

// The warning marker next to an already-installed mod: a gold triangle-exclamation
// (the app's FontAwesome icon set) and a hoverable inline target for the Tooltip.
const WarningMarker = styled.span`
  display: inline-flex;
  align-items: center;
  flex: 0 0 auto;
  color: #faad14;
  cursor: help;
`;

type FacetKey = 'settings' | 'config';

type Props = {
  rows: UserDataModRow[];
  state: UserDataSelectionState;
  onChange: (state: UserDataSelectionState) => void;
  // Whether the app-settings row can be toggled. For import over an archive that
  // carries none, it renders disabled and unchecked ("not in this archive").
  appSettingsAvailable: boolean;
  disabled?: boolean;
  // Mods that importing would overwrite (already installed). Each gets a warning
  // marker next to its row. Unused by export.
  overwriteModIds?: Set<string>;
};

export function UserDataSelectionForm({
  rows,
  state,
  onChange,
  appSettingsAvailable,
  disabled,
  overwriteModIds,
}: Props) {
  const { t } = useTranslation();

  const settingsLabel = t('settings.userData.settings');
  const configLabel = t('settings.userData.config');

  // Size the facet columns to the wider of the two titles (plus a margin) so neither
  // wraps. On the first pass the measurer can still be unlaid-out (the modal may be
  // mid-mount), which would read 0, so measure through a ResizeObserver that fires again
  // once it has a real box and whenever a title changes, e.g. on a language switch.
  const measureRef = useRef<HTMLDivElement>(null);
  const [facetColumnWidth, setFacetColumnWidth] = useState<number>();
  useLayoutEffect(() => {
    const el = measureRef.current;
    if (!el) {
      return;
    }
    const measure = () => {
      let widest = 0;
      for (const child of Array.from(el.children)) {
        widest = Math.max(widest, child.getBoundingClientRect().width);
      }
      if (widest > 0) {
        setFacetColumnWidth(Math.ceil(widest) + FACET_COLUMN_MARGIN);
      }
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [settingsLabel, configLabel]);

  const setAllIncluded = (included: boolean) => {
    onChange(
      produce(state, (draft) => {
        for (const row of rows) {
          const rowState = draft.perMod[row.modId];
          if (rowState) {
            rowState.included = included;
          }
        }
      })
    );
  };

  const setAllFacet = (facet: FacetKey, value: boolean) => {
    onChange(
      produce(state, (draft) => {
        for (const row of rows) {
          if (facet === 'settings' ? row.canSettings : row.canConfig) {
            const rowState = draft.perMod[row.modId];
            if (rowState) {
              rowState[facet] = value;
            }
          }
        }
      })
    );
  };

  const setRow = (
    modId: string,
    patch: Partial<UserDataSelectionState['perMod'][string]>
  ) => {
    onChange(
      produce(state, (draft) => {
        const rowState = draft.perMod[modId];
        if (rowState) {
          Object.assign(rowState, patch);
        }
      })
    );
  };

  const includedCount = rows.filter(
    (row) => state.perMod[row.modId]?.included
  ).length;
  const allIncluded = rows.length > 0 && includedCount === rows.length;
  const someIncluded = includedCount > 0 && includedCount < rows.length;

  const facetMaster = (facet: FacetKey) => {
    const capableRows = rows.filter((row) =>
      facet === 'settings' ? row.canSettings : row.canConfig
    );
    const onCount = capableRows.filter(
      (row) => state.perMod[row.modId]?.[facet]
    ).length;
    return {
      available: capableRows.length > 0,
      checked: capableRows.length > 0 && onCount === capableRows.length,
      indeterminate: onCount > 0 && onCount < capableRows.length,
    };
  };

  const settingsMaster = facetMaster('settings');
  const configMaster = facetMaster('config');

  return (
    <FormWrapper>
      <FacetMeasurer ref={measureRef} aria-hidden="true">
        <span>{settingsLabel}</span>
        <span>{configLabel}</span>
      </FacetMeasurer>

      <Checkbox
        data-testid="user-data-app-settings"
        checked={appSettingsAvailable && state.appSettings}
        disabled={disabled || !appSettingsAvailable}
        onChange={(e) => onChange({ ...state, appSettings: e.target.checked })}
      >
        {t('settings.userData.appSettings')}
        {!appSettingsAvailable && (
          <Tag style={{ marginLeft: 8 }}>
            {t('settings.userData.notInArchive')}
          </Tag>
        )}
      </Checkbox>

      <ModsSection>
        <SectionLabel>{t('settings.userData.mods')}</SectionLabel>
        <ModList
          style={
            facetColumnWidth
              ? ({
                  '--facet-column-width': `${facetColumnWidth}px`,
                } as CSSProperties)
              : undefined
          }
        >
          <HeaderRow>
            <IncludeCell>
              <Checkbox
                data-testid="user-data-include-all"
                checked={allIncluded}
                indeterminate={someIncluded}
                disabled={disabled || rows.length === 0}
                onChange={(e) => setAllIncluded(e.target.checked)}
              >
                {t('general.contextMenu.selectAll')}
              </Checkbox>
            </IncludeCell>
            <FacetLabel>{settingsLabel}</FacetLabel>
            <FacetLabel>{configLabel}</FacetLabel>
          </HeaderRow>

          {rows.length === 0 ? (
            <EmptyNotice>{t('settings.userData.noMods')}</EmptyNotice>
          ) : (
            <>
              <ModRow>
                <IncludeCell style={{ color: 'var(--whui-text-muted)' }}>
                  {t('settings.userData.applyToAll')}
                </IncludeCell>
                <FacetCell>
                  <Checkbox
                    data-testid="user-data-facet-all"
                    data-facet="settings"
                    checked={settingsMaster.checked}
                    indeterminate={settingsMaster.indeterminate}
                    disabled={disabled || !settingsMaster.available}
                    onChange={(e) => setAllFacet('settings', e.target.checked)}
                  />
                </FacetCell>
                <FacetCell>
                  <Checkbox
                    data-testid="user-data-facet-all"
                    data-facet="config"
                    checked={configMaster.checked}
                    indeterminate={configMaster.indeterminate}
                    disabled={disabled || !configMaster.available}
                    onChange={(e) => setAllFacet('config', e.target.checked)}
                  />
                </FacetCell>
              </ModRow>

              {rows.map((row) => {
                const rowState = state.perMod[row.modId];
                const included = !!rowState?.included;
                return (
                  <ModRow
                    key={row.modId}
                    data-testid="user-data-row"
                    data-mod-id={row.modId}
                  >
                    <IncludeCell>
                      <Checkbox
                        data-testid="user-data-include"
                        checked={included}
                        disabled={disabled}
                        onChange={(e) =>
                          setRow(row.modId, { included: e.target.checked })
                        }
                      >
                        <ModName title={getDisplayModId(row.modId)}>{row.name}</ModName>
                      </Checkbox>
                      {row.isLocal && (
                        <Tag color="blue">{t('settings.userData.local')}</Tag>
                      )}
                      {overwriteModIds?.has(row.modId) && (
                        <Tooltip title={t('settings.userData.alreadyInstalled')}>
                          <WarningMarker>
                            <FontAwesomeIcon icon={faTriangleExclamation} />
                          </WarningMarker>
                        </Tooltip>
                      )}
                    </IncludeCell>
                    <FacetCell>
                      <FacetCheckbox
                        facet="settings"
                        available={row.canSettings}
                        checked={row.canSettings && !!rowState?.settings}
                        disabled={disabled || !included}
                        onChange={(value) =>
                          setRow(row.modId, { settings: value })
                        }
                      />
                    </FacetCell>
                    <FacetCell>
                      <FacetCheckbox
                        facet="config"
                        available={row.canConfig}
                        checked={row.canConfig && !!rowState?.config}
                        disabled={disabled || !included}
                        onChange={(value) =>
                          setRow(row.modId, { config: value })
                        }
                      />
                    </FacetCell>
                  </ModRow>
                );
              })}
            </>
          )}
        </ModList>
      </ModsSection>
    </FormWrapper>
  );
}

// A facet checkbox, or - when the archive does not carry that facet for the mod - a
// disabled placeholder with a "not in this archive" tooltip.
function FacetCheckbox({
  facet,
  available,
  checked,
  disabled,
  onChange,
}: {
  facet: FacetKey;
  available: boolean;
  checked: boolean;
  disabled?: boolean;
  onChange: (value: boolean) => void;
}) {
  const { t } = useTranslation();
  if (!available) {
    return (
      <Tooltip title={t('settings.userData.notInArchive')}>
        <Checkbox
          data-testid="user-data-facet"
          data-facet={facet}
          checked={false}
          disabled
        />
      </Tooltip>
    );
  }
  return (
    <Checkbox
      data-testid="user-data-facet"
      data-facet={facet}
      checked={checked}
      disabled={disabled}
      onChange={(e) => onChange(e.target.checked)}
    />
  );
}
