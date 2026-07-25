import { Button, type ButtonProps } from 'antd';
import React from 'react';
import { type To, useHref, useLinkClickHandler } from 'react-router-dom';

interface Props {
  onClick?: React.MouseEventHandler<HTMLElement>;
  replace?: boolean;
  state?: unknown;
  target?: React.HTMLAttributeAnchorTarget;
  to: To;
}

// https://github.com/remix-run/react-router/pull/7998
function ButtonLink({
  onClick,
  replace = false,
  state,
  target,
  to,
  ...rest
}: Props & ButtonProps) {
  const href = useHref(to);
  const handleClick = useLinkClickHandler(to, { replace, state, target });

  return (
    <Button
      {...rest}
      href={href}
      onClick={(event) => {
        onClick?.(event);
        if (!event.defaultPrevented) {
          handleClick(event as React.MouseEvent<HTMLAnchorElement, MouseEvent>);
        }
      }}
      target={target}
    />
  );
}

export default ButtonLink;
