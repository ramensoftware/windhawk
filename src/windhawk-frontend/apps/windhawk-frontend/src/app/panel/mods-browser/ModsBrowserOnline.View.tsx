import { AppUISettingsContext } from '@app/appUISettings';
import { DropdownModal, InputWithContextMenu } from '@app/components/InputWithContextMenu';
import { isMobile } from '@app/utils';
import type { ModConfig, ModMetadata, RepositoryDetails } from '@app/webviewIPCMessages';
import { faFilter, faSearch, faSort } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, Empty, type InputRef, Modal, Result, Spin } from 'antd';
import { useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useInfiniteScroll } from 'react-infinite-scroll-component';
import { useBlocker, useNavigate } from 'react-router-dom';
import styled, { css } from 'styled-components';
import { ModDetails } from '../mod-details';
import { ModCard } from '../shared';

// Use webpack constant for conditional compilation
declare const WEBPACK_IS_WEBSITE: boolean;

const MODS_PATH = WEBPACK_IS_WEBSITE ? '/mods' : '/mods-browser';

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

const SearchFilterContainer = styled.div`
  display: flex;
  gap: 10px;
  margin: 20px 0;
`;

const SearchFilterInput = styled(InputWithContextMenu)`
  > .ant-input-prefix {
    margin-inline-end: 8px;
  }
`;

const IconButton = styled(Button)`
  padding-inline-start: 0;
  padding-inline-end: 0;
  min-width: 40px;
`;

const ModsContainer = styled.div<{ $extraBottomPadding?: boolean }>`
  flex: 1;
  ${({ $extraBottomPadding }) => css`
    padding-bottom: ${$extraBottomPadding ? 70 : 20}px;
  `}
`;

const ResultsMessageWrapper = styled.div`
  margin-top: 85px;
`;

const ModsGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(
    auto-fill,
    calc(min(400px - 20px * 4 / 3, 100%))
  );
  gap: 20px;
  justify-content: center;
`;

const ProgressSpin = styled(Spin)`
  display: block;
  margin-inline-start: auto;
  margin-inline-end: auto;
  font-size: 32px;
`;

const FilterItemLabelWrapper = styled.span`
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
`;

interface FilterItemLabelProps {
  label: string;
  count?: number;
}

// Formats a count for display:
// 1, 2, ..., 99, 100, ..., 999, 1K, 1.1K, ..., 9.8K, 9.9K, 10K+
const formatBadgeCount = (count: number): string => {
  if (count < 1000) {
    return count.toString();
  }
  if (count < 10000) {
    // Floor to the nearest 100 so e.g. 9999 stays "9.9K" rather than rounding
    // up to "10K", which is reserved for 10000+.
    return `${Math.floor(count / 100) / 10}K`;
  }
  return '10K+';
};

const FilterItemLabel = ({ label, count }: FilterItemLabelProps) => (
  <FilterItemLabelWrapper>
    <span>{label}</span>
    {count !== undefined && (
      <Badge
        count={formatBadgeCount(count)}
        title={count.toString()}
        color='var(--whui-skeleton-base)'
        style={{
          color: 'var(--whui-text-secondary)',
          boxShadow: 'none',
          height: '18px',
          lineHeight: '18px',
          minWidth: '18px',
          padding: '0 6px',
        }}
        overflowCount={Infinity}
      />
    )}
  </FilterItemLabelWrapper>
);

const normalizeProcessName = (process: string): string => {
  return process.includes('\\')
    ? process.substring(process.lastIndexOf('\\') + 1)
    : process;
};

/**
 * Extracts valid process names from a mod's include array for filtering/counting.
 * Handles wildcards consistently: "*" is kept as-is, paths with wildcards in
 * the directory are normalized to filenames, filenames with wildcards are skipped.
 */
const extractValidProcesses = (include: string[]): string[] => {
  const validProcesses: string[] = [];

  for (const process of include) {
    if (!process) {
      continue;
    }

    // Include "*" as-is (matches all processes)
    if (process === '*') {
      validProcesses.push('*');
      continue;
    }

    // Extract the filename from the path
    const normalized = normalizeProcessName(process);

    // Skip if the filename itself contains wildcards
    if (normalized.includes('*') || normalized.includes('?')) {
      continue;
    }

    validProcesses.push(normalized);
  }

  return validProcesses;
};

const extractItemsWithCounts = <TMod,>(
  repositoryMods: Record<string, TMod> | null,
  keyPrefix: string,
  extractItems: (mod: TMod) => string[]
) => {
  if (!repositoryMods) {
    return [];
  }

  const itemCounts = new Map<string, { count: number; casings: Map<string, number> }>();

  for (const mod of Object.values(repositoryMods)) {
    const items = extractItems(mod);

    // Deduplicate items within this mod to count each mod only once per item
    const seenLowerItems = new Set<string>();

    for (const item of items) {
      if (!item) {
        continue;
      }

      const lowerItem = item.toLowerCase();

      // Skip if we've already counted this item for this mod
      if (seenLowerItems.has(lowerItem)) {
        continue;
      }
      seenLowerItems.add(lowerItem);

      const existing = itemCounts.get(lowerItem);
      if (existing) {
        existing.count++;
        const casingCount = existing.casings.get(item);
        existing.casings.set(item, (casingCount || 0) + 1);
      } else {
        const casings = new Map<string, number>();
        casings.set(item, 1);
        itemCounts.set(lowerItem, { count: 1, casings });
      }
    }
  }

  return Array.from(itemCounts.entries())
    .map(([lowerName, { count, casings }]) => {
      // Find the most common casing, or first lexicographically if tied
      const displayName = Array.from(casings.entries()).reduce(
        (best, [casing, casingCount]) => {
          if (casingCount > best.count || (casingCount === best.count && casing < best.casing)) {
            return { casing, count: casingCount };
          }
          return best;
        },
        { casing: '', count: 0 }
      ).casing;

      return {
        name: displayName,
        count,
        key: `${keyPrefix}:${lowerName}`,
        lowerName,
      };
    })
    .sort((a, b) => {
      if (b.count !== a.count) {
        return b.count - a.count;
      }
      return a.lowerName.localeCompare(b.lowerName);
    });
};

export const extractAuthorsWithCounts = <TMod,>(
  repositoryMods: Record<string, TMod> | null,
  getModMetadata: (mod: TMod) => ModMetadata
) => {
  return extractItemsWithCounts(
    repositoryMods,
    'author',
    (mod) => {
      const metadata = getModMetadata(mod);
      return metadata.author ? [metadata.author] : [];
    }
  );
};

export const extractProcessesWithCounts = <TMod,>(
  repositoryMods: Record<string, TMod> | null,
  getModMetadata: (mod: TMod) => ModMetadata
) => {
  return extractItemsWithCounts(
    repositoryMods,
    'process',
    (mod) => {
      const metadata = getModMetadata(mod);
      return extractValidProcesses(metadata.include || []);
    }
  );
};

const useFilterState = () => {
  const [filterText, setFilterText] = useState('');
  const [filterOptions, setFilterOptions] = useState<Set<string>>(new Set());
  const [filterDropdownOpen, setFilterDropdownOpen] = useState(false);
  const [showAllAuthors, setShowAllAuthors] = useState(false);
  const [showAllProcesses, setShowAllProcesses] = useState(false);
  const [searchInputFocused, setSearchInputFocused] = useState(!isMobile);

  const handleFilterChange = useCallback((key: string) => {
    setFilterOptions((prevOptions) => {
      const newOptions = new Set(prevOptions);

      // Handle mutually exclusive filters for installation status
      if (key === 'installed' && newOptions.has('not-installed')) {
        newOptions.delete('not-installed');
      } else if (key === 'not-installed' && newOptions.has('installed')) {
        newOptions.delete('installed');
      }

      // Toggle the clicked option
      if (newOptions.has(key)) {
        newOptions.delete(key);
      } else {
        newOptions.add(key);
      }

      return newOptions;
    });
  }, []);

  const handleClearFilters = useCallback(() => {
    setFilterOptions(new Set());
    setShowAllAuthors(false);
    setShowAllProcesses(false);
  }, []);

  return {
    filterText,
    setFilterText,
    filterOptions,
    filterDropdownOpen,
    setFilterDropdownOpen,
    showAllAuthors,
    setShowAllAuthors,
    showAllProcesses,
    setShowAllProcesses,
    searchInputFocused,
    setSearchInputFocused,
    handleFilterChange,
    handleClearFilters,
  };
};

export interface ModsBrowserOnlineViewProps<TMod> {
  ContentWrapper: React.ComponentType<
    React.ComponentPropsWithoutRef<'div'> & { $hidden?: boolean }
  >;
  repositoryMods: Record<string, TMod> | null;
  initialDataPending: boolean;
  displayedModId?: string;

  // Accessor functions - each mode provides its own
  getModMetadata: (mod: TMod) => ModMetadata;
  getModMetadataEnglish: (mod: TMod) => ModMetadata | undefined;
  getModDetails: (mod: TMod) => RepositoryDetails;
  getInstalledDetails?: (mod: TMod) => { metadata: ModMetadata | null; config: ModConfig | null; userRating?: number } | undefined;

  // Whether installation status filter should be shown (extension mode only)
  showInstallationFilter?: boolean;

  // Extension-only (optional)
  installModPending?: boolean;
  compileModPending?: boolean;
  installModContext?: { updating: boolean };

  // Retry handler (different per mode)
  onRetry?: () => void;

  // ModDetails props (extension-only)
  modDetailsExtensionProps?: {
    installedModDetails?: {
      metadata: ModMetadata | null;
      config: ModConfig | null;
      userRating?: number;
    };
    loadRepositoryData: boolean;
    installMod: (modSource: string) => void;
    updateMod: (modSource: string) => void;
    forkModFromSource: (modSource: string) => void;
    compileMod: () => void;
    enableMod: (enable: boolean) => void;
    editMod: () => void;
    forkMod: () => void;
    deleteMod: () => void;
    updateModRating: (newRating: number) => void;
  };
}

export function ModsBrowserOnlineView<TMod>(props: ModsBrowserOnlineViewProps<TMod>) {
  const {
    ContentWrapper,
    repositoryMods,
    initialDataPending,
    displayedModId,
    getModMetadata,
    getModMetadataEnglish,
    getModDetails,
    getInstalledDetails,
    showInstallationFilter,
    installModPending,
    compileModPending,
    installModContext,
    onRetry,
    modDetailsExtensionProps,
  } = props;

  const { t } = useTranslation();
  const navigate = useNavigate();

  // UI state managed internally
  const [sortingOrder, setSortingOrder] = useState('popular-top-rated');
  const [infiniteScrollLoadedItems, setInfiniteScrollLoadedItems] = useState(30);
  const [detailsButtonClicked, setDetailsButtonClicked] = useState(false);

  // Filter state
  const {
    filterText,
    setFilterText,
    filterOptions,
    filterDropdownOpen,
    setFilterDropdownOpen,
    showAllAuthors,
    setShowAllAuthors,
    showAllProcesses,
    setShowAllProcesses,
    searchInputFocused,
    setSearchInputFocused,
    handleFilterChange,
    handleClearFilters,
  } = useFilterState();

  const { devModeOptOut } = useContext(AppUISettingsContext);

  const searchInputRef = useRef<InputRef>(null);

  // Keyboard shortcut: "/" to focus search (desktop only)
  useEffect(() => {
    if (isMobile) {
      return;
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't trigger if user is typing in an input, textarea, or contenteditable
      const target = e.target as HTMLElement;
      if (
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable
      ) {
        return;
      }

      if (e.key === '/') {
        e.preventDefault();
        searchInputRef.current?.focus();
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, []);

  const resetInfiniteScrollLoadedItems = () => setInfiniteScrollLoadedItems(30);

  // Extract filter data
  const authorFilters = useMemo(
    () => extractAuthorsWithCounts(repositoryMods, getModMetadata),
    [repositoryMods, getModMetadata]
  );

  const processFilters = useMemo(
    () => extractProcessesWithCounts(repositoryMods, getModMetadata),
    [repositoryMods, getModMetadata]
  );

  const installedModsFilteredAndSorted = useMemo(() => {
    const filterWords = filterText.toLowerCase().split(/\s+/)
      .map(word => word.trim())
      .filter(word => word.length > 0);
    return Object.entries(repositoryMods || {})
      .filter(([modId, mod]) => {
        const metadata = getModMetadata(mod);
        const metadataEnglish = getModMetadataEnglish(mod);

        // Apply text filter
        if (filterWords.length > 0) {
          const textMatch = filterWords.every((filterWord) => {
            return (
              modId.toLowerCase().includes(filterWord) ||
              metadata.name?.toLowerCase().includes(filterWord) ||
              metadata.description?.toLowerCase().includes(filterWord) ||
              metadataEnglish?.name?.toLowerCase().includes(filterWord) ||
              metadataEnglish?.description?.toLowerCase().includes(filterWord)
            );
          });
          if (!textMatch) {
            return false;
          }
        }

        // Apply category filters - if none selected, show all
        if (filterOptions.size === 0) {
          return true;
        }

        // Collect selected authors and processes
        const selectedAuthors: string[] = [];
        const selectedProcesses: string[] = [];
        let installedFilter: boolean | null = null;

        for (const key of filterOptions) {
          if (key.startsWith('author:')) {
            selectedAuthors.push(key.substring('author:'.length));
          } else if (key.startsWith('process:')) {
            selectedProcesses.push(key.substring('process:'.length));
          } else if (key === 'installed') {
            installedFilter = true;
          } else if (key === 'not-installed') {
            installedFilter = false;
          }
        }

        // Check installation status filter
        if (installedFilter !== null) {
          const isInstalled = getInstalledDetails?.(mod) !== undefined;
          if (isInstalled !== installedFilter) {
            return false;
          }
        }

        // Check author filter (OR logic within authors)
        if (selectedAuthors.length > 0) {
          const author = metadata.author?.toLowerCase();
          if (!author || !selectedAuthors.some(a => a === author)) {
            return false;
          }
        }

        // Check process filter (OR logic within processes)
        // Uses extractValidProcesses to ensure consistency with counting logic
        if (selectedProcesses.length > 0) {
          const processes = extractValidProcesses(metadata.include || [])
            .map(p => p.toLowerCase());
          if (!selectedProcesses.some(sp => processes.includes(sp))) {
            return false;
          }
        }

        return true;
      })
      .sort((a, b) => {
        const [modIdA, modA] = a;
        const [modIdB, modB] = b;
        const detailsA = getModDetails(modA);
        const detailsB = getModDetails(modB);

        switch (sortingOrder) {
          case 'popular-top-rated':
            if (detailsB.defaultSorting < detailsA.defaultSorting) {
              return -1;
            } else if (detailsB.defaultSorting > detailsA.defaultSorting) {
              return 1;
            }
            break;

          case 'popular':
            if (detailsB.users < detailsA.users) {
              return -1;
            } else if (detailsB.users > detailsA.users) {
              return 1;
            }
            break;

          case 'top-rated':
            if (detailsB.rating < detailsA.rating) {
              return -1;
            } else if (detailsB.rating > detailsA.rating) {
              return 1;
            }
            break;

          case 'newest':
            if (detailsB.published < detailsA.published) {
              return -1;
            } else if (detailsB.published > detailsA.published) {
              return 1;
            }
            break;

          case 'last-updated':
            if (detailsB.updated < detailsA.updated) {
              return -1;
            } else if (detailsB.updated > detailsA.updated) {
              return 1;
            }
            break;

          case 'alphabetical':
            // Nothing to do.
            break;
        }

        // Fallback sorting: Sort by name, then id.
        const metadataA = getModMetadata(modA);
        const metadataB = getModMetadata(modB);

        const modATitle = (metadataA.name || modIdA).toLowerCase();
        const modBTitle = (metadataB.name || modIdB).toLowerCase();

        if (modATitle < modBTitle) {
          return -1;
        } else if (modATitle > modBTitle) {
          return 1;
        }

        if (modIdA < modIdB) {
          return -1;
        } else if (modIdA > modIdB) {
          return 1;
        }

        return 0;
      });
  }, [repositoryMods, sortingOrder, filterText, filterOptions, getModMetadata, getModMetadataEnglish, getModDetails, getInstalledDetails]);

  const { sentinelRef } = useInfiniteScroll({
    dataLength: infiniteScrollLoadedItems,
    next: () =>
      setInfiniteScrollLoadedItems((prev) =>
        Math.min(prev + 30, installedModsFilteredAndSorted.length)
      ),
    hasMore: infiniteScrollLoadedItems < installedModsFilteredAndSorted.length,
    scrollableTarget: 'ModsBrowserOnline-ContentWrapper',
  });

  // IntersectionObserver fires only when the sentinel transitions across
  // threshold 0; holding End keeps it continuously inside the trigger zone so
  // the hook stops firing. A scroll listener catches each browser-applied
  // scroll (one per End keypress) and chains the next batch.
  const totalCount = installedModsFilteredAndSorted.length;
  useEffect(() => {
    const root = document.getElementById('ModsBrowserOnline-ContentWrapper');
    if (!root) {
      return;
    }
    const handleScroll = () => {
      if (root.scrollHeight - (root.scrollTop + root.clientHeight) > root.clientHeight) {
        return;
      }
      setInfiniteScrollLoadedItems((prev) =>
        prev < totalCount ? Math.min(prev + 30, totalCount) : prev
      );
    };
    root.addEventListener('scroll', handleScroll, { passive: true });
    return () => root.removeEventListener('scroll', handleScroll);
  }, [totalCount]);

  // Block all navigation when modal is open
  const modalIsOpen = !!(installModPending || compileModPending);

  useBlocker(({ currentLocation, nextLocation }) => {
    return modalIsOpen && currentLocation.pathname !== nextLocation.pathname;
  });

  if (initialDataPending) {
    return (
      <CenteredContainer>
        <CenteredContent>
          <ProgressSpin size="large" tip={t('general.status.loading')} />
        </CenteredContent>
      </CenteredContainer>
    );
  }

  if (!repositoryMods) {
    return (
      <CenteredContainer>
        <CenteredContent>
          <Result
            status="error"
            title={t('general.status.loadingFailedTitle')}
            subTitle={t('general.status.loadingFailedSubtitle')}
            extra={
              onRetry
                ? [
                  <Button
                    type="primary"
                    key="try-again"
                    onClick={onRetry}
                  >
                    {t('general.status.tryAgain')}
                  </Button>,
                ]
                : []
            }
          />
        </CenteredContent>
      </CenteredContainer>
    );
  }

  return (
    <>
      <ContentWrapper
        id="ModsBrowserOnline-ContentWrapper"
        $hidden={!!displayedModId}
      >
        <ModsContainer $extraBottomPadding={!devModeOptOut}>
          <SearchFilterContainer>
            <SearchFilterInput
              ref={searchInputRef}
              autoFocus={!isMobile}
              data-testid="mods-search"
              prefix={<FontAwesomeIcon icon={faSearch} />}
              placeholder={t(isMobile || searchInputFocused ? 'modSearch.placeholder' : 'modSearch.placeholderWithHint') as string}
              allowClear
              value={filterText}
              onChange={(e) => {
                resetInfiniteScrollLoadedItems();
                setFilterText(e.target.value);
              }}
              onFocus={() => setSearchInputFocused(true)}
              onBlur={() => setSearchInputFocused(false)}
            />
            <DropdownModal
              placement="bottomRight"
              trigger={['click']}
              arrow={true}
              open={filterDropdownOpen}
              onOpenChange={setFilterDropdownOpen}
              menu={{
                style: { maxHeight: '400px', overflowY: 'overlay' },
                items: [
                  ...(showInstallationFilter ? [{
                    type: 'group' as const,
                    label: t('explore.filter.installationStatus'),
                    children: [
                      {
                        label: t('explore.filter.installed'),
                        key: 'installed',
                      },
                      {
                        label: t('explore.filter.notInstalled'),
                        key: 'not-installed',
                      },
                    ],
                  }] : []),
                  {
                    type: 'group',
                    label: t('explore.filter.author'),
                    children: [
                      ...(showAllAuthors ? authorFilters : authorFilters.slice(0, 5)).map(author => ({
                        label: <FilterItemLabel label={author.name} count={author.count} />,
                        key: author.key,
                      })),
                      ...(authorFilters.length > 5 && !showAllAuthors ? [{
                        label: t('explore.filter.showMore'),
                        key: 'show-more-authors',
                      }] : []),
                    ],
                  },
                  {
                    type: 'group',
                    label: t('explore.filter.process'),
                    children: [
                      ...(showAllProcesses ? processFilters : processFilters.slice(0, 5)).map(process => ({
                        label: <FilterItemLabel label={process.name} count={process.count} />,
                        key: process.key,
                      })),
                      ...(processFilters.length > 5 && !showAllProcesses ? [{
                        label: t('explore.filter.showMore'),
                        key: 'show-more-processes',
                      }] : []),
                    ],
                  },
                  {
                    type: 'divider',
                  },
                  {
                    label: t('explore.filter.clearFilters'),
                    key: 'clear-filters',
                  },
                ],
                selectedKeys: Array.from(filterOptions),
                onClick: (e) => {
                  if (e.key === 'clear-filters') {
                    handleClearFilters();
                    setFilterDropdownOpen(false);
                    resetInfiniteScrollLoadedItems();
                  } else if (e.key === 'show-more-authors') {
                    setShowAllAuthors(true);
                  } else if (e.key === 'show-more-processes') {
                    setShowAllProcesses(true);
                  } else {
                    handleFilterChange(e.key);
                    resetInfiniteScrollLoadedItems();
                    // Keep dropdown open for filter changes
                  }
                },
              }}
            >
              <IconButton
                type={filterOptions.size > 0 ? 'primary' : undefined}
                data-testid="mods-filter"
              >
                <FontAwesomeIcon icon={faFilter} />
              </IconButton>
            </DropdownModal>
            <DropdownModal
              placement="bottomRight"
              trigger={['click']}
              arrow={true}
              menu={{
                items: [
                  {
                    label: t('explore.search.popularAndTopRated'),
                    key: 'popular-top-rated',
                  },
                  { label: t('explore.search.popular'), key: 'popular' },
                  { label: t('explore.search.topRated'), key: 'top-rated' },
                  { label: t('explore.search.newest'), key: 'newest' },
                  {
                    label: t('explore.search.lastUpdated'),
                    key: 'last-updated',
                  },
                  {
                    label: t('explore.search.alphabeticalOrder'),
                    key: 'alphabetical',
                  },
                ],
                selectedKeys: [sortingOrder],
                onClick: (e) => {
                  resetInfiniteScrollLoadedItems();
                  setSortingOrder(e.key);
                },
              }}
            >
              <IconButton data-testid="mods-sort">
                <FontAwesomeIcon icon={faSort} />
              </IconButton>
            </DropdownModal>
          </SearchFilterContainer>
          {installedModsFilteredAndSorted.length === 0 ? (
            <ResultsMessageWrapper>
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t('modSearch.noResults')}
              />
            </ResultsMessageWrapper>
          ) : (
            <>
              <ModsGrid data-testid="repository-mods">
                {installedModsFilteredAndSorted
                  .slice(0, infiniteScrollLoadedItems)
                  .map(([modId, mod]) => {
                    // Normalize mod structure using helper functions
                    const modMetadata = getModMetadata(mod);
                    const repositoryDetails = getModDetails(mod);
                    const installedDetails = getInstalledDetails?.(mod);

                    return (
                      <ModCard
                        key={modId}
                        modId={modId}
                        ribbonText={
                          installedDetails
                            ? installedDetails.metadata?.version !== modMetadata.version
                              ? (t('mod.updateAvailable') as string)
                              : (t('mod.installed') as string)
                            : undefined
                        }
                        title={modMetadata.name || modId}
                        description={modMetadata.description}
                        modMetadata={modMetadata}
                        repositoryDetails={repositoryDetails}
                        buttons={[
                          /// #if WEBSITE
                          {
                            type: 'navigate',
                            text: t('mod.details'),
                            testId: 'mod-card-details',
                            href: MODS_PATH + '/' + modId,
                            onClick: () => setDetailsButtonClicked(true),
                          },
                          /// #else
                          {
                            type: 'action',
                            text: t('mod.details'),
                            testId: 'mod-card-details',
                            onClick: () => {
                              setDetailsButtonClicked(true);
                              navigate(MODS_PATH + '/' + modId);
                            },
                          },
                          /// #endif
                        ]}
                      />
                    );
                  })}
              </ModsGrid>
              <div ref={sentinelRef as React.RefObject<HTMLDivElement>} aria-hidden="true" />
            </>
          )}
        </ModsContainer>
      </ContentWrapper>
      {displayedModId && repositoryMods && repositoryMods[displayedModId] && (
        <ContentWrapper>
          <ModDetails
            modId={displayedModId}
            repositoryModDetails={{
              metadata: getModMetadata(repositoryMods[displayedModId]),
              details: getModDetails(repositoryMods[displayedModId]),
            }}
            goBack={() => {
              // If we ever clicked on Details, go back.
              // Otherwise, we probably arrived from a different location,
              // go straight to the mods page.
              if (detailsButtonClicked) {
                navigate(-1);
              } else {
                navigate(MODS_PATH);
              }
            }}
            // Only pass extensionProps in extension mode
            extensionProps={modDetailsExtensionProps}
          />
        </ContentWrapper>
      )}
      {(installModPending || compileModPending) && (
        <Modal open={true} closable={false} footer={null}>
          <ProgressSpin
            size="large"
            tip={
              installModPending
                ? installModContext?.updating
                  ? t('general.status.updating')
                  : t('general.status.installing')
                : compileModPending
                  ? t('general.status.compiling')
                  : ''
            }
          />
        </Modal>
      )}
    </>
  );
}
