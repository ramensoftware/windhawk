import { shuffleArray } from '@app/utils';
import { fetchCatalogJson } from '@app/utils/swrHelpers';
import type { ModMetadata, RepositoryDetails } from '@app/webviewIPCMessages';
import { faDiscord, faGithubAlt } from '@fortawesome/free-brands-svg-icons';
import { faLink, faRocket, faStar } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Empty, Spin, Tooltip } from 'antd';
import { useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import styled, { css, keyframes } from 'styled-components';
import useSWR from 'swr';
import landingMainScreenshot from './assets/windhawk-screenshot-main.png';
import { ModCard } from './shared';
import ButtonLink from './shared/ButtonLink';

const LandingSection = styled.div`
  display: flex;
  flex-direction: column;
  align-items: center;
  margin-top: 40px;
`;

const LandingMainText = styled.div`
  font-size: 72px;
  font-family: Oxanium;

  @media (max-width: 576px) {
    font-size: 48px;
  }
`;

const LandingMainDescription = styled.div`
  font-size: 24px;
  text-align: center;

  @media (max-width: 576px) {
    font-size: 20px;
  }
`;

const LandingMainActions = styled.div`
  display: flex;
  column-gap: 30px;
  margin: 30px 0;

  @media (max-width: 576px) {
    flex-direction: column;
    row-gap: 20px;
  }
`;

const GetInvolvedContent = styled.div`
  display: flex;
  flex-direction: column;
  row-gap: 8px;

  a {
    height: auto;
    text-align: left;
    padding: 0 15px 4px !important;
  }
`;

const GetInvolvedFontAwesomeIconContainer = styled.span`
  width: 28px;
  vertical-align: middle;
  line-height: 1.15;
`;

const GetInvolvedDescription = styled.span`
  white-space: normal;
  width: calc(100% - 28px);
  vertical-align: middle;
  line-height: 1.15;
`;

const MainScreenshotImageAnimation = keyframes`
  from {
  }

  to {
    filter: drop-shadow(0 0 4px #fff) drop-shadow(0 0 5px var(--whui-primary)) drop-shadow(0 0 8px var(--whui-primary)) brightness(1.1);
  }
`;

const MainScreenshotImage = styled.img`
  &:hover {
    animation: ${MainScreenshotImageAnimation} 2s ease-in-out infinite alternate;
  }
`;

const SectionText = styled.h2`
  margin-top: 40px;
`;

const SectionIcon = styled(FontAwesomeIcon)`
  margin-right: 3px;
`;

const HighlightsSection = styled.div`
  display: flex;
  column-gap: 20px;

  @media (max-width: 576px) {
    flex-direction: column;
    row-gap: 20px;
  }
`;

const HighlightsItem = styled.div`
  flex: 1;
`;

const HighlightsItemTitle = styled.h3`
`;

const HighlightsItemText = styled.div`
  color: var(--whui-text-muted);
`;

const ModsContainer = styled.div<{ $extraBottomPadding?: boolean }>`
  flex: 1;
  ${({ $extraBottomPadding }) => css`
    padding-bottom: ${$extraBottomPadding ? 70 : 20}px;
  `}
`;

const ModsGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(auto-fill, calc(min(400px - 20px * 4 / 3, 100%)));
  gap: 20px;
  justify-content: center;
`;

const ExploreModsButton = styled(ButtonLink)`
  display: flex;
  flex-direction: column;
  justify-content: center;
  height: 100%;
  font-size: 22px;
`;

const ProgressSpin = styled(Spin)`
  display: block;
  margin-left: auto;
  margin-right: auto;
  font-size: 32px;
`;

type ModDetailsType = {
  metadata: ModMetadata;
  details: RepositoryDetails;
  featured: boolean;
};

type OnlineCatalogType = {
  mods: Record<string, ModDetailsType>;
};

function GetInvolvedPopup({ t }: { t: (key: string) => string }) {
  return (
    <GetInvolvedContent>
      <Button
        size='large'
        ghost
        href='https://discord.com/servers/windhawk-923944342991818753'
      >
        <GetInvolvedFontAwesomeIconContainer>
          <FontAwesomeIcon icon={faDiscord} />
        </GetInvolvedFontAwesomeIconContainer>
        <GetInvolvedDescription>
          {t('website.home.joinDiscord')}
        </GetInvolvedDescription>
      </Button>
      <Button
        size='large'
        ghost
        href='https://github.com/ramensoftware/windhawk/discussions'
      >
        <GetInvolvedFontAwesomeIconContainer>
          <FontAwesomeIcon icon={faGithubAlt} />
        </GetInvolvedFontAwesomeIconContainer>
        <GetInvolvedDescription>
          {t('website.home.discussWindhawk')}
        </GetInvolvedDescription>
      </Button>
      <Button
        size='large'
        ghost
        href='https://github.com/ramensoftware/windhawk-mods/discussions'
      >
        <GetInvolvedFontAwesomeIconContainer>
          <FontAwesomeIcon icon={faGithubAlt} />
        </GetInvolvedFontAwesomeIconContainer>
        <GetInvolvedDescription>
          {t('website.home.discussMods')}
        </GetInvolvedDescription>
      </Button>
      <ButtonLink
        size='large'
        ghost
        to='/links'
      >
        <GetInvolvedFontAwesomeIconContainer>
          <FontAwesomeIcon icon={faLink} />
        </GetInvolvedFontAwesomeIconContainer>
        <GetInvolvedDescription>
          {t('website.home.browseLinks')}
        </GetInvolvedDescription>
      </ButtonLink>
    </GetInvolvedContent>
  );
}

interface Props {
  ContentWrapper: React.ComponentType<
    React.ComponentPropsWithoutRef<'div'> & { $hidden?: boolean }
  >;
}

function WebsiteHome({ ContentWrapper }: Props) {
  const { t, i18n } = useTranslation();

  useEffect(() => {
    document.title = 'Windhawk';
  }, []);

  // Fetch catalog from web with language-specific fallback
  const { data: onlineCatalog, error: onlineCatalogError, isLoading } = useSWR<OnlineCatalogType>(
    ['catalog', i18n.language],
    () => fetchCatalogJson(i18n.language)
  );
  const repositoryMods = isLoading ? undefined : onlineCatalogError ? null : onlineCatalog?.mods;

  const featuredMods = useMemo(() => {
    if (!repositoryMods) {
      return repositoryMods;
    }

    return Object.entries(repositoryMods).filter(([modId, mod]) => mod.featured);
  }, [repositoryMods]);

  const featuredModsShuffled = useMemo(() => {
    return featuredMods && shuffleArray([...featuredMods]);
  }, [featuredMods]);

  const featuredModsFilteredAndSorted = useMemo(() => {
    const maxFeaturedModsToShow = 5;
    return featuredModsShuffled && featuredModsShuffled.slice(0, maxFeaturedModsToShow);
  }, [featuredModsShuffled]);

  return (
    <ContentWrapper>
      <ModsContainer>
        <LandingSection>
          <LandingMainText>Windhawk</LandingMainText>
          <LandingMainDescription>
            {t('website.home.tagline')}
          </LandingMainDescription>
          <LandingMainActions>
            <Button
              type='primary'
              size='large'
              href='https://ramensoftware.com/downloads/windhawk_setup.exe'
            >
              {t('general.actions.download')}
            </Button>
            <ButtonLink
              type='primary'
              size='large'
              to='/mods'
            >
              {t('home.browse')}
            </ButtonLink>
            <Tooltip
              title={<GetInvolvedPopup t={t} />}
              placement='bottom'
              trigger='click'
              overlayStyle={{ maxWidth: '80vw' }}
              // Scroll with container: https://github.com/ant-design/ant-design/issues/25117#issuecomment-873747921
              getPopupContainer={triggerNode => triggerNode.parentElement || document.body}
            >
              <Button
                type='primary'
                size='large'
              >
                {t('website.home.getInvolved')}
              </Button>
            </Tooltip>
          </LandingMainActions>
          <MainScreenshotImage src={landingMainScreenshot} alt='Windhawk screenshot' />
        </LandingSection>
        <SectionText>
          <SectionIcon icon={faRocket} /> {t('website.home.highlights')}
        </SectionText>
        <HighlightsSection>
          <HighlightsItem>
            <HighlightsItemTitle>{t('website.home.robust')}</HighlightsItemTitle>
            <HighlightsItemText>
              {t('website.home.robustDescription')}
            </HighlightsItemText>
          </HighlightsItem>
          <HighlightsItem>
            <HighlightsItemTitle>{t('website.home.simple')}</HighlightsItemTitle>
            <HighlightsItemText>
              {t('website.home.simpleDescription')}
            </HighlightsItemText>
          </HighlightsItem>
          <HighlightsItem>
            <HighlightsItemTitle>{t('website.home.transparent')}</HighlightsItemTitle>
            <HighlightsItemText>
              {t('website.home.transparentDescription')}
            </HighlightsItemText>
          </HighlightsItem>
        </HighlightsSection>
        <SectionText>
          <SectionIcon icon={faStar} /> {t('home.featuredMods.title')}
        </SectionText>
        {featuredModsFilteredAndSorted === undefined
          ?
          <ProgressSpin
            size="large"
            tip={t('general.status.loading')}
          />
          :
          featuredModsFilteredAndSorted === null
            ?
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('general.status.loadingFailed')}
            />
            :
            <ModsGrid>
              {featuredModsFilteredAndSorted.map(([modId, mod]) => (
                <ModCard
                  key={modId}
                  modId={modId}
                  title={mod.metadata.name || modId}
                  description={mod.metadata.description}
                  modMetadata={mod.metadata}
                  repositoryDetails={mod.details}
                  buttons={[
                    {
                      type: 'navigate',
                      text: t('mod.details'),
                      testId: 'mod-card-details',
                      href: '/mods/' + modId,
                    },
                  ]}
                />
              ))}
              <ExploreModsButton size="large" to="/mods">
                {t('home.featuredMods.explore')}
              </ExploreModsButton>
            </ModsGrid>
        }
      </ModsContainer>
    </ContentWrapper>
  );
}

export default WebsiteHome;
