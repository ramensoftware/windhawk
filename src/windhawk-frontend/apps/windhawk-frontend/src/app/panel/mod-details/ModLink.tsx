import {
  faArrowUpRightFromSquare,
  faCheck,
  faCopy,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Popover, Spin } from 'antd';
import { type ReactNode, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';
import styled from 'styled-components';
import { useCopyToClipboard } from '@app/panel/shared/useCopyToClipboard';
import { MOD_PAGE_FROM_APP_STATE, MODS_PATH } from '../mods-browser/modsPath';
import { useRepositoryModName } from './useRepositoryModName';

// A pointer crossing the text is not asking about every link in it: the
// popover, and the read of the host its first open can make, wait for it to
// settle.
const OPEN_DELAY_SECONDS = 0.3;

const PopoverBody = styled.div`
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-width: 320px;
`;

const ModName = styled.div`
  font-weight: 600;
  overflow-wrap: anywhere;
`;

const Actions = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
`;

// What tells a mod link from the links around it at rest: a mark of its kind
// ahead of the text, in the link's own color. An outline cube drawn here at
// the weight of the letterforms, since a filled icon at text size is a blob
// beside the word. Spaced by a margin rather than a space so a wrap cannot
// leave the mark on a line of its own.
const ModGlyph = styled.svg.attrs({
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinejoin: 'round',
  'aria-hidden': true,
})`
  width: 0.95em;
  height: 0.95em;
  vertical-align: -0.1em;
  margin-inline-end: 0.3em;
`;

// Mounted on the popover's first open, which is when the name is looked up.
function ModLinkPopoverContent({ modId, href }: { modId: string; href: string }) {
  const { t } = useTranslation();

  const { name, isLoading } = useRepositoryModName(modId);

  const { copied, copy } = useCopyToClipboard();

  return (
    <PopoverBody data-testid="mod-link-popover">
      {/* A mod the listing does not carry is named by the one thing the link
          says about it. */}
      <ModName>{isLoading ? <Spin size="small" /> : (name ?? modId)}</ModName>
      <Actions>
        <Button
          size="small"
          href={href}
          target="_blank"
          data-testid="mod-link-open-in-browser"
        >
          <FontAwesomeIcon icon={faArrowUpRightFromSquare} />
          {t('modDetails.modLink.openInBrowser')}
        </Button>
        <Button
          size="small"
          data-testid="mod-link-copy"
          onClick={() => copy(href)}
        >
          <FontAwesomeIcon icon={copied ? faCheck : faCopy} />
          {copied
            ? t('modDetails.modLink.copied')
            : t('modDetails.modLink.copyLink')}
        </Button>
      </Actions>
    </PopoverBody>
  );
}

interface Props {
  modId: string;
  // The page's address as the document wrote it, which is what the browser is
  // handed and what the copy takes.
  href: string;
  children: ReactNode;
}

/**
 * A link to a mod's page on windhawk.net, drawn in the app: marked as a mod, a
 * click opens the mod here, with the way back leading to the page the link
 * was on, and hovering names the mod and offers the page itself, in the
 * browser or as an address to paste elsewhere.
 *
 * The anchor's own href is the app's route for the mod, not the page's: the
 * VSCode webview host opens every followed http link in the browser, whether
 * or not the click was handled here.
 */
export function ModLink({ modId, href, children }: Props) {
  // Closed by the click, ahead of the navigation: the document the link is in
  // is replaced by the mod's, and an anchor drawn in the same place in it would
  // inherit an open popover.
  const [open, setOpen] = useState(false);

  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      mouseEnterDelay={OPEN_DELAY_SECONDS}
      content={<ModLinkPopoverContent modId={modId} href={href} />}
    >
      <Link
        to={`${MODS_PATH}/${modId}`}
        state={MOD_PAGE_FROM_APP_STATE}
        data-testid="mod-link"
        data-mod-id={modId}
        onClick={() => setOpen(false)}
      >
        <ModGlyph data-testid="mod-link-glyph">
          <path d="M12 2.5 21 7.5v9L12 21.5 3 16.5v-9z" />
          <path d="M3 7.5l9 5 9-5M12 12.5v9" />
        </ModGlyph>
        {children}
      </Link>
    </Popover>
  );
}

export default ModLink;
