import { Typography } from 'antd';
import { TooltipPlacement } from 'antd/lib/tooltip';
import { useEffect, useRef, useState } from 'react';

interface Props extends React.PropsWithChildren {
  className?: string;
  style?: React.CSSProperties;
  tooltipPlacement?: TooltipPlacement;
}

/**
 * A text component that automatically shows a tooltip when truncated.
 * Uses ResizeObserver to recalculate ellipsis on width changes.
 * Automatically hides tooltip when resizing to prevent stale tooltip display.
 */
function EllipsisText(props: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [tooltipHide, setTooltipHide] = useState(false);
  const [ellipsisKey, setEllipsisKey] = useState(0);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) {
      return;
    }

    const resizeObserver = new ResizeObserver(() => {
      // Prevent tooltip from being shown when ellipsis appears
      setTooltipHide(true);
      // Trigger ellipsis recalculation by changing the key
      setEllipsisKey((prev) => prev + 1);
    });

    resizeObserver.observe(element);

    return () => {
      resizeObserver.disconnect();
    };
  }, []);

  return (
    <Typography.Text
      ref={containerRef}
      className={props.className}
      style={props.style}
      ellipsis={{
        tooltip: {
          placement: props.tooltipPlacement,
          onOpenChange: (visible) => {
            if (visible) {
              setTooltipHide(false);
            }
          },
          ... (
            tooltipHide ? { open: false } : {}
          )
        },
      }}
    >
      {props.children}
      {/* Change key to force ellipsis recalculation on resize */}
      <span key={ellipsisKey}></span>
    </Typography.Text>
  );
}

export default EllipsisText;
