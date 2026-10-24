import { faEllipsis } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, type ButtonProps, Dropdown, type MenuProps, Space } from 'antd';
import { type ReactNode } from 'react';

interface Props {
  type?: ButtonProps['type'];
  size?: ButtonProps['size'];
  // Of the action half alone: the menu half stays usable either way.
  disabled?: boolean;
  loading?: boolean;
  block?: boolean;
  menu: MenuProps;
  onClick?: () => void;
  children: ReactNode;
}

// An action button joined to a button that opens a menu of related actions.
export function SplitButton({
  type,
  size,
  disabled,
  loading,
  block,
  menu,
  onClick,
  children,
}: Props) {
  return (
    <Space.Compact block={block}>
      <Button
        type={type}
        size={size}
        disabled={disabled}
        loading={loading}
        block={block}
        onClick={onClick}
      >
        {children}
      </Button>
      <Dropdown menu={menu} placement="bottomRight">
        <Button
          type={type}
          size={size}
          icon={<FontAwesomeIcon icon={faEllipsis} />}
        />
      </Dropdown>
    </Space.Compact>
  );
}
