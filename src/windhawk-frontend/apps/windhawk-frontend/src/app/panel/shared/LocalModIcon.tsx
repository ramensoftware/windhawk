import styled from 'styled-components';

import localModIcon from '../assets/local-mod-icon.svg';

// The asset is a solid white glyph, which disappears on the light theme's background, so
// it is painted as a mask tinted with currentColor rather than drawn as an image - the
// glyph then follows the text color of whatever surface it sits on. Forced colors rewrites
// background-color to Canvas, which would erase it, but honors the system color keywords
// as authored, so restate the tint from CanvasText there.
const LocalModIcon = styled.span.attrs({ role: 'img' })`
  display: inline-block;
  flex: none;
  width: 24px;
  height: 24px;
  background-color: currentColor;
  -webkit-mask: url(${localModIcon}) no-repeat center / contain;
  mask: url(${localModIcon}) no-repeat center / contain;
  cursor: help;

  @media (forced-colors: active) {
    background-color: CanvasText;
  }
`;

export default LocalModIcon;
