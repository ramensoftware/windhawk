import { faGithubAlt } from '@fortawesome/free-brands-svg-icons';
import {
  faBullhorn,
  faCrosshairs,
  faHome,
  faUser,
  IconDefinition,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, ConfigProvider, Divider, Tooltip, Typography } from 'antd';
import { useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { sanitizeUrl } from '../utils';
import { ModMetadata, RepositoryDetails } from '../webviewIPCMessages';

type TranslationFunction = ReturnType<typeof useTranslation>['t'];

const MetadataLineWrapper = styled.div<{ $singleLine?: boolean }>`
  display: flex;
  flex-wrap: ${({ $singleLine }) => ($singleLine ? 'nowrap' : 'wrap')};
  margin-top: 4px;
  margin-bottom: 2px;
`;

const MetadataItemWrapper = styled.div<{ $width?: number; $singleLine?: boolean }>`
  font-size: 14px;
  font-weight: normal;
  overflow: hidden;
  ${({ $singleLine }) => $singleLine && `
    // Don't shrink automatically; widths are managed manually.
    flex-shrink: 0;
  `}
  ${({ $width }) => $width !== undefined && `
    width: ${$width}px;
  `}
`;

const MetadataLineIcon = styled(FontAwesomeIcon)`
  margin-inline-end: 3px;
`;

const TextAsIconWrapper = styled.span`
  font-size: 18px;
  line-height: 18px;
  user-select: none;
`;

const VersionTooltipHeader = styled.div`
  text-align: center;
`;

const VersionTooltipGrid = styled.div`
  display: grid;
  grid-template-columns: auto auto;
  gap: 4px 8px;
  margin-top: 8px;
`;

const VersionTooltipLabel = styled.div`
  text-align: end;
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
  color: #388ed3;
`;

const TooltipNote = styled.div`
  margin-top: 12px;
  padding-top: 8px;
  border-top: 1px solid rgba(255, 255, 255, 0.2);
  font-size: 12px;
  color: rgba(255, 255, 255, 0.65);
`;

interface MetadataItem {
  key: string;
  icon: IconDefinition;
  text: string;
  tooltip: string | React.ReactNode;
  showBadge?: boolean;
}

interface CustomProcesses {
  include: string[];
  exclude: string[];
  includeExcludeCustomOnly: boolean;
};

function createVersionItem(
  version: string,
  t: TranslationFunction,
  repositoryDetails?: RepositoryDetails
): MetadataItem {
  let tooltip: React.ReactNode = t('modDetails.header.modVersion');

  if (repositoryDetails) {
    const updatedDate = new Date(repositoryDetails.updated);

    const formatDate = (date: Date) => {
      return date.toLocaleDateString('en-US', {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
      });
    };

    tooltip = (
      <>
        <VersionTooltipHeader>{t('modDetails.header.modVersion')}</VersionTooltipHeader>
        <VersionTooltipGrid>
          <VersionTooltipLabel>{t('modDetails.header.lastUpdated')}:</VersionTooltipLabel>
          <div>{formatDate(updatedDate)}</div>
        </VersionTooltipGrid>
      </>
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
      <div>{t('modDetails.header.modAuthor.title')}</div>
      {(modMetadata.homepage ||
        modMetadata.github ||
        modMetadata.twitter) && (
          <div>
            {modMetadata.homepage && (
              <Tooltip
                title={t('modDetails.header.modAuthor.homepage')}
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
                title={t('modDetails.header.modAuthor.github')}
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
                title={t('modDetails.header.modAuthor.twitter')}
                placement="bottom"
              >
                <Button
                  type="text"
                  icon={<TextAsIconWrapper>𝕏</TextAsIconWrapper>}
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
  let tooltip: string | React.ReactNode;

  if (include.length === 1 && exclude.length === 0) {
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

  if (include.length === 1 && exclude.length === 0 && !customProcesses) {
    // Simple case: single process, use simple tooltip
    tooltip = t('modDetails.header.processes.tooltip.target');
  } else {
    const isCustomOnly = customProcesses?.includeExcludeCustomOnly ?? false;

    tooltip = (
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
          {(customProcesses?.include ?? []).map((process, i) => {
            return (
              <li key={i}>
                <CustomProcessItem>{process}</CustomProcessItem>
              </li>
            );
          })}
        </TooltipProcessList>
        {(exclude.length > 0 || (customProcesses?.exclude ?? []).length > 0) && (
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
              {(customProcesses?.exclude ?? []).map((process, i) => {
                return (
                  <li key={i}>
                    <CustomProcessItem>{process}</CustomProcessItem>
                  </li>
                );
              })}
            </TooltipProcessList>
          </>
        )}
        {customProcesses && (
          <TooltipNote>
            <CustomProcessItem>●</CustomProcessItem> {t('modDetails.header.processes.tooltip.customListsNote')}
          </TooltipNote>
        )}
      </>
    );
  }

  return {
    key: 'processes',
    icon: faCrosshairs,
    text,
    tooltip,
    showBadge: customProcesses !== undefined,
  };
}

function buildMetadataItems(
  modMetadata: ModMetadata,
  t: TranslationFunction,
  customProcesses?: CustomProcesses,
  repositoryDetails?: RepositoryDetails
): MetadataItem[] {
  const items: MetadataItem[] = [];

  if (modMetadata.version) {
    items.push(createVersionItem(modMetadata.version, t, repositoryDetails));
  }

  if (modMetadata.author) {
    items.push(createAuthorItem(modMetadata.author, modMetadata, t));
  }

  if (modMetadata.include && modMetadata.include.length > 0) {
    items.push(createProcessesItem(modMetadata, t, customProcesses));
  }

  return items;
}

// Width constraints for single-line mode
const PROCESSES_MIN_WIDTH = 50;

interface ItemWidths {
  [key: string]: number | undefined;
}

/**
 * Calculates constrained widths for metadata items based on priority:
 * 1. Version: capped at half of container width, never shrinks
 * 2. Processes: shrinks first, down to PROCESSES_MIN_WIDTH
 * 3. Author: shrinks last, gets remaining space
 */
function calculateItemWidths(
  containerWidth: number,
  naturalWidths: Record<string, number>
): ItemWidths {
  const totalNaturalWidth = Object.values(naturalWidths).reduce(
    (sum, width) => sum + width, 0);

  // If everything fits naturally, no constraints needed
  if (totalNaturalWidth <= containerWidth) {
    return {};
  }

  const versionNatural = naturalWidths['version'] || 0;
  const authorNatural = naturalWidths['author'] || 0;
  const processesNatural = naturalWidths['processes'] || 0;

  // Version: capped at max, never shrinks below natural (up to cap)
  const versionWidth = Math.min(versionNatural, containerWidth / 2);
  let remainingWidth = containerWidth - versionWidth;

  // Processes: shrinks first, down to minimum
  const processesWidth = Math.max(
    PROCESSES_MIN_WIDTH,
    Math.min(processesNatural, remainingWidth - authorNatural)
  );
  remainingWidth -= processesWidth;

  // Author: gets remaining space (may shrink significantly)
  const authorWidth = Math.max(0, remainingWidth);

  return {
    'version': versionWidth,
    'author': authorWidth,
    'processes': processesWidth,
  };
}

interface Props {
  modMetadata: ModMetadata;
  singleLine?: boolean;
  customProcesses?: CustomProcesses;
  repositoryDetails?: RepositoryDetails;
}

function ModMetadataLine(props: Props) {
  const { t } = useTranslation();
  const { modMetadata, singleLine, customProcesses, repositoryDetails } = props;

  const { direction } = useContext(ConfigProvider.ConfigContext);

  const metadataItems = useMemo(
    () => buildMetadataItems(
      modMetadata,
      t,
      (
        (customProcesses?.include || []).length > 0 ||
        (customProcesses?.exclude || []).length > 0 ||
        customProcesses?.includeExcludeCustomOnly
      ) ? customProcesses : undefined,
      repositoryDetails
    ),
    [modMetadata, t, customProcesses, repositoryDetails]
  );

  const containerRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<{ [key: string]: HTMLDivElement | null }>({});
  const textRefs = useRef<{ [key: string]: HTMLElement | null }>({});
  const [itemWidths, setItemWidths] = useState<ItemWidths>({});
  const [textWidths, setTextWidths] = useState<ItemWidths>({});

  const measureAndCalculate = useCallback(() => {
    if (!singleLine || !containerRef.current) {
      setItemWidths({});
      setTextWidths({});
      return;
    }

    // Skip if element is not visible
    const containerRect = containerRef.current.getBoundingClientRect();
    if (containerRect.width === 0 || containerRect.height === 0) {
      return;
    }

    const containerWidth = containerRect.width;

    const naturalWidths: Record<string, number> = {};
    const naturalTextWidths: Record<string, number> = {};
    for (const item of metadataItems) {
      const el = itemRefs.current[item.key];
      const textEl = textRefs.current[item.key];
      if (el && textEl) {
        // Temporarily set width to 'auto' to measure natural width, including
        // the hidden overflow text, then restore. Similar to el.scrollWidth, but
        // fractional, which is important for accurate total width and for
        // avoiding ellipsis.
        const prevElWidth = el.style.width;
        const prevTextElWidth = textEl.style.width;
        el.style.width = 'auto';
        textEl.style.width = 'auto';
        naturalWidths[item.key] = el.getBoundingClientRect().width;
        naturalTextWidths[item.key] = textEl.getBoundingClientRect().width;
        el.style.width = prevElWidth;
        textEl.style.width = prevTextElWidth;
      } else {
        naturalWidths[item.key] = 0;
        naturalTextWidths[item.key] = 0;
      }
    }

    const calculatedWidths = calculateItemWidths(
      containerWidth,
      naturalWidths,
    );

    // Calculate text widths based on the difference between item and text
    // natural widths
    const calculatedTextWidths: ItemWidths = {};
    for (const item of metadataItems) {
      const itemWidth = calculatedWidths[item.key];
      if (itemWidth !== undefined) {
        const widthDifference = naturalWidths[item.key] - naturalTextWidths[item.key];
        calculatedTextWidths[item.key] = itemWidth - widthDifference;
      }
    }

    setItemWidths(calculatedWidths);
    setTextWidths(calculatedTextWidths);
  }, [singleLine, metadataItems]);

  // Use useLayoutEffect for synchronous measurement before paint
  useLayoutEffect(() => {
    if (!singleLine) {
      return;
    }

    // Initial measurement
    measureAndCalculate();
  }, [singleLine, measureAndCalculate]);

  useEffect(() => {
    if (!singleLine) {
      return;
    }

    // Set up ResizeObserver for container size changes
    const resizeObserver = new ResizeObserver(() => {
      measureAndCalculate();
    });

    if (containerRef.current) {
      resizeObserver.observe(containerRef.current);
    }

    return () => {
      resizeObserver.disconnect();
    };
  }, [singleLine, measureAndCalculate]);

  if (metadataItems.length === 0) {
    return null;
  }

  return (
    <MetadataLineWrapper ref={containerRef} $singleLine={singleLine}>
      {metadataItems.map((item, i) => (
        <MetadataItemWrapper
          key={item.key}
          ref={(el) => {
            itemRefs.current[item.key] = el;
          }}
          $width={itemWidths[item.key]}
          $singleLine={singleLine}
        >
          {/* Single-line: divider before item (except first) */}
          {singleLine && i !== 0 && <Divider type="vertical" />}

          <Tooltip
            title={item.tooltip}
            placement="bottom"
          >
            <Typography.Text
              ref={(el) => {
                textRefs.current[item.key] = el;
              }}
              style={{ width: textWidths[item.key] }}
              ellipsis={true}
            >
              <Badge
                dot={item.showBadge}
                offset={[direction === 'rtl' ? 4 : -4, 4]}
                color="#177ddc"
              >
                <MetadataLineIcon icon={item.icon} />
              </Badge>
              {item.text}
            </Typography.Text>
          </Tooltip>

          {/* Multi-line: divider after item (except last) */}
          {!singleLine && i < metadataItems.length - 1 && <Divider type="vertical" />}
        </MetadataItemWrapper>
      ))}
    </MetadataLineWrapper>
  );
}

export default ModMetadataLine;
