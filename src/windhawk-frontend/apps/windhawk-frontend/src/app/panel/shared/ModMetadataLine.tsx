import { singleLineEllipsis } from '@app/components/EllipsisText';
import { formatShortDate, sanitizeUrl } from '@app/utils';
import { type ModMetadata, type RepositoryDetails } from '@app/webviewIPCMessages';
import { faGithubAlt, faXTwitter } from '@fortawesome/free-brands-svg-icons';
import {
  faBullhorn,
  faCrosshairs,
  faHome,
  faUser,
  type IconDefinition,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, ConfigProvider, Divider, Tooltip, Typography } from 'antd';
import { useContext, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';

type TranslationFunction = ReturnType<typeof useTranslation>['t'];

const MetadataLineWrapper = styled.div<{ $singleLine?: boolean }>`
  display: flex;
  flex-wrap: ${({ $singleLine }) => ($singleLine ? 'nowrap' : 'wrap')};
  margin-top: 4px;
  margin-bottom: 2px;
`;

// How the single line shares out its width when the items do not all fit: the
// version keeps its own, up to half the line; the processes take what the
// others leave, down to a minimum; and past that minimum the author shrinks.
const singleLineItemSizing = {
  version: css`
    flex: 0 0 auto;
    max-width: 50%;
  `,
  author: css`
    flex: 0 1 auto;
  `,
  processes: css`
    flex: 1 1 0;
    min-width: 50px;
    max-width: max-content;
  `,
};

// Single-line, each item is a flex row of its own too, so that its text shrinks
// to the room the divider leaves it.
const MetadataItemWrapper = styled.div<{
  $singleLine?: boolean;
  $itemKey: MetadataItem['key'];
}>`
  font-size: 14px;
  font-weight: normal;
  overflow: hidden;
  ${({ $singleLine, $itemKey }) =>
    $singleLine &&
    css`
      display: flex;
      ${singleLineItemSizing[$itemKey]}
    `}
`;

// The divider in a box of its own, so that it stays aligned to the line of text
// by its vertical-align rather than becoming a flex item of the row.
const MetadataItemDivider = styled.span`
  flex: none;
`;

const MetadataText = styled(Typography.Text)`
  ${singleLineEllipsis}
`;

const MetadataLineIcon = styled(FontAwesomeIcon)`
  margin-inline-end: 3px;
`;

const VersionTooltip = styled.div`
  text-align: center;
`;

const VersionTooltipUpdated = styled.div`
  margin-top: 8px;
`;

const VersionTooltipDate = styled.span`
  white-space: nowrap;
`;

const TooltipProcessList = styled.ul`
  margin: 4px 0;
  padding-inline-start: 20px;
`;

const TooltipSection = styled.div<{ $hasMarginTop?: boolean }>`
  ${({ $hasMarginTop }) => $hasMarginTop && 'margin-top: 8px;'}
`;

const DisabledProcessItem = styled.span`
  text-decoration: line-through;
  opacity: 0.5;
`;

const CustomProcessItem = styled.span`
  color: var(--whui-link);
`;

const TooltipNote = styled.div`
  margin-top: 12px;
  padding-top: 8px;
  border-top: 1px solid var(--whui-border-faint);
`;

const TooltipNoteList = styled.ul`
  margin: 0;
  padding-inline-start: 20px;
  color: var(--whui-link);
`;

const TooltipNoteText = styled.div`
  color: var(--whui-text-muted);
  font-size: 12px;
`;

interface MetadataItem {
  key: 'version' | 'author' | 'processes';
  icon: IconDefinition;
  text: string;
  tooltip: string | React.ReactNode;
  showBadge?: boolean;
}

interface CustomProcesses {
  include: string[];
  exclude: string[];
  includeExcludeCustomOnly: boolean;
  patternsMatchCriticalSystemProcesses: boolean;
};

function createVersionItem(
  version: string,
  t: TranslationFunction,
  language: string,
  repositoryDetails?: RepositoryDetails
): MetadataItem {
  let tooltip: React.ReactNode = t('modDetails.header.modVersion');

  if (repositoryDetails) {
    const updatedDate = new Date(repositoryDetails.updated);

    // The line breaks only after the colon, keeping the date whole.
    tooltip = (
      <VersionTooltip>
        <div>{t('modDetails.header.modVersion')}</div>
        <VersionTooltipUpdated>
          {t('modDetails.header.lastUpdated')}: <VersionTooltipDate>{formatShortDate(updatedDate, language)}</VersionTooltipDate>
        </VersionTooltipUpdated>
      </VersionTooltip>
    );
  }

  return {
    key: 'version',
    icon: faBullhorn,
    text: version,
    tooltip,
  };
}

function createAuthorTooltip(
  modMetadata: ModMetadata,
  t: TranslationFunction
): React.ReactNode {
  return (
    <>
      <div>{t('general.modAuthor.title')}</div>
      {(modMetadata.homepage ||
        modMetadata.github ||
        modMetadata.twitter) && (
          <div>
            {modMetadata.homepage && (
              <Tooltip
                title={t('general.modAuthor.homepage')}
                placement="bottom"
              >
                <Button
                  type="text"
                  icon={<FontAwesomeIcon icon={faHome} />}
                  href={sanitizeUrl(modMetadata.homepage)}
                />
              </Tooltip>
            )}
            {modMetadata.github && (
              <Tooltip
                title={t('general.modAuthor.github')}
                placement="bottom"
              >
                <Button
                  type="text"
                  icon={<FontAwesomeIcon icon={faGithubAlt} />}
                  href={sanitizeUrl(modMetadata.github)}
                />
              </Tooltip>
            )}
            {modMetadata.twitter && (
              <Tooltip
                title={t('general.modAuthor.twitter')}
                placement="bottom"
              >
                <Button
                  type="text"
                  icon={<FontAwesomeIcon icon={faXTwitter} />}
                  href={sanitizeUrl(modMetadata.twitter)}
                />
              </Tooltip>
            )}
          </div>
        )}
    </>
  );
}

function createAuthorItem(
  author: string,
  modMetadata: ModMetadata,
  t: TranslationFunction
): MetadataItem {
  return {
    key: 'author',
    icon: faUser,
    text: author,
    tooltip: createAuthorTooltip(modMetadata, t),
  };
}

function createProcessesItem(
  modMetadata: ModMetadata,
  t: TranslationFunction,
  customProcesses?: CustomProcesses
): MetadataItem {
  const include = modMetadata.include || [];
  const exclude = modMetadata.exclude || [];
  let text: string;

  if (include.length === 0) {
    text = '';
  } else if (include.length === 1 && exclude.length === 0) {
    if (include[0] === '*') {
      text = t('modDetails.header.processes.all');
    } else {
      text = include[0];
    }
  } else {
    if (include.length === 1 && include[0] === '*') {
      text = t('modDetails.header.processes.allBut', {
        list: exclude.join(', '),
      });
    } else if (exclude.length > 0) {
      text = t('modDetails.header.processes.except', {
        included: include.join(', '),
        excluded: exclude.join(', '),
      });
    } else {
      text = include.join(', ');
    }
  }

  const includeCustom = customProcesses?.include || [];
  const excludeCustom = customProcesses?.exclude || [];
  const isCustomOnly = customProcesses?.includeExcludeCustomOnly ?? false;
  const patternsMatchCriticalSystemProcesses = customProcesses?.patternsMatchCriticalSystemProcesses ?? false;

  const hasCustomLists = includeCustom.length > 0 || excludeCustom.length > 0 || isCustomOnly;

  const tooltip = (
    <>
      <TooltipSection><strong>{t('modDetails.header.processes.tooltip.targets')}</strong></TooltipSection>
      <TooltipProcessList>
        {include.map((process, i) => {
          return (
            <li key={i}>
              {isCustomOnly ? (
                <DisabledProcessItem>{process}</DisabledProcessItem>
              ) : (
                process
              )}
            </li>
          );
        })}
        {includeCustom.map((process, i) => {
          return (
            <li key={i}>
              <CustomProcessItem>{process}</CustomProcessItem>
            </li>
          );
        })}
      </TooltipProcessList>
      {(exclude.length > 0 || excludeCustom.length > 0) && (
        <>
          <TooltipSection $hasMarginTop><strong>{t('modDetails.header.processes.tooltip.excluded')}</strong></TooltipSection>
          <TooltipProcessList>
            {exclude.map((process, i) => {
              return (
                <li key={i}>
                  {isCustomOnly ? (
                    <DisabledProcessItem>{process}</DisabledProcessItem>
                  ) : (
                    process
                  )}
                </li>
              );
            })}
          </TooltipProcessList>
          <TooltipProcessList>
            {excludeCustom.map((process, i) => {
              return (
                <li key={i}>
                  <CustomProcessItem>{process}</CustomProcessItem>
                </li>
              );
            })}
          </TooltipProcessList>
        </>
      )}
      {(hasCustomLists || patternsMatchCriticalSystemProcesses) && (
        <TooltipNote>
          <TooltipNoteList>
            {hasCustomLists && (
              <li><TooltipNoteText>{t('modDetails.header.processes.tooltip.customListsNote')}</TooltipNoteText></li>
            )}
            {patternsMatchCriticalSystemProcesses && (
              <li><TooltipNoteText>{t('modDetails.header.processes.tooltip.patternsMatchCriticalSystemProcessesNote')}</TooltipNoteText></li>
            )}
          </TooltipNoteList>
        </TooltipNote>
      )}
    </>
  );

  return {
    key: 'processes',
    icon: faCrosshairs,
    text,
    tooltip,
    showBadge: hasCustomLists || patternsMatchCriticalSystemProcesses,
  };
}

function buildMetadataItems(
  modMetadata: ModMetadata,
  t: TranslationFunction,
  language: string,
  customProcesses?: CustomProcesses,
  repositoryDetails?: RepositoryDetails
): MetadataItem[] {
  const items: MetadataItem[] = [];

  if (modMetadata.version) {
    items.push(createVersionItem(modMetadata.version, t, language, repositoryDetails));
  }

  if (modMetadata.author) {
    items.push(createAuthorItem(modMetadata.author, modMetadata, t));
  }

  if ((modMetadata?.include || []).length > 0 ||
    (customProcesses?.include || []).length > 0) {
    items.push(createProcessesItem(modMetadata, t, customProcesses));
  }

  return items;
}

interface Props {
  modMetadata: ModMetadata;
  singleLine?: boolean;
  customProcesses?: CustomProcesses;
  repositoryDetails?: RepositoryDetails;
}

function ModMetadataLine(props: Props) {
  const { t, i18n } = useTranslation();
  const { modMetadata, singleLine, customProcesses, repositoryDetails } = props;

  const { direction } = useContext(ConfigProvider.ConfigContext);

  const metadataItems = useMemo(
    () => buildMetadataItems(
      modMetadata,
      t,
      i18n.language,
      customProcesses,
      repositoryDetails
    ),
    [modMetadata, t, i18n.language, customProcesses, repositoryDetails]
  );

  if (metadataItems.length === 0) {
    return null;
  }

  return (
    <MetadataLineWrapper $singleLine={singleLine}>
      {metadataItems.map((item, i) => (
        <MetadataItemWrapper
          key={item.key}
          $singleLine={singleLine}
          $itemKey={item.key}
        >
          {/* Single-line: divider before item (except first) */}
          {singleLine && i !== 0 && (
            <MetadataItemDivider>
              <Divider orientation="vertical" />
            </MetadataItemDivider>
          )}

          <Tooltip
            title={item.tooltip}
            placement="bottom"
          >
            <MetadataText>
              <Badge
                dot={item.showBadge}
                offset={[direction === 'rtl' ? 4 : -4, 4]}
                color="var(--whui-primary)"
              >
                <MetadataLineIcon icon={item.icon} />
              </Badge>
              {item.text}
            </MetadataText>
          </Tooltip>

          {/* Multi-line: divider after item (except last) */}
          {!singleLine && i < metadataItems.length - 1 && <Divider orientation="vertical" />}
        </MetadataItemWrapper>
      ))}
    </MetadataLineWrapper>
  );
}

export default ModMetadataLine;
