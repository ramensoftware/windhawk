import { Tooltip, type TooltipProps, Typography } from 'antd';
import { useRef, useState } from 'react';
import styled, { css } from 'styled-components';

// antd's single-line ellipsis as CSS alone. Typography's `ellipsis` prop also
// measures the text after every render and keeps an IntersectionObserver on
// it, which adds up wherever many texts are on screen.
export const singleLineEllipsis = css`
  display: inline-block;
  max-width: 100%;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  vertical-align: bottom;
`;

// Set in its context's type rather than in antd's base size, like the text it
// stands in for.
const EllipsisTypographyText = styled(Typography.Text)`
  ${singleLineEllipsis}
  font-size: inherit;
  line-height: inherit;
`;

interface Props extends React.PropsWithChildren {
  className?: string;
  style?: React.CSSProperties;
  tooltipPlacement?: TooltipProps['placement'];
}

/**
 * Single-line text cut off with an ellipsis, with a tooltip showing all of it
 * when it is cut off. That is checked when the tooltip is about to open, so
 * nothing is measured or observed while the text is only on screen.
 */
function EllipsisText(props: Props) {
  const textRef = useRef<HTMLSpanElement>(null);
  const [tooltipOpen, setTooltipOpen] = useState(false);

  return (
    <Tooltip
      title={props.children}
      placement={props.tooltipPlacement}
      open={tooltipOpen}
      onOpenChange={(open) => {
        const text = textRef.current;
        setTooltipOpen(open && !!text && text.scrollWidth > text.clientWidth);
      }}
    >
      <EllipsisTypographyText
        ref={textRef}
        className={props.className}
        style={props.style}
      >
        {props.children}
      </EllipsisTypographyText>
    </Tooltip>
  );
}

export default EllipsisText;
