import {
  Dropdown,
  DropdownProps,
  Input,
  InputNumber,
  InputNumberProps,
  MenuProps,
  Popconfirm,
  PopconfirmProps,
  Select,
  SelectProps,
} from 'antd';
import { InputProps, InputRef, TextAreaProps } from 'antd/lib/input';
import { TextAreaRef } from 'antd/lib/input/TextArea';
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';

function useItems() {
  const { t } = useTranslation();

  const items: MenuProps['items'] = useMemo(
    () => [
      {
        label: t('general.cut'),
        key: 'cut',
      },
      {
        label: t('general.copy'),
        key: 'copy',
      },
      {
        label: t('general.paste'),
        key: 'paste',
      },
      {
        type: 'divider',
      },
      {
        label: t('general.selectAll'),
        key: 'selectAll',
      },
    ],
    [t]
  );

  return items;
}

function onClick(
  textArea: HTMLTextAreaElement | HTMLInputElement | null | undefined,
  key: string
) {
  if (textArea) {
    textArea.focus();
    document.execCommand(key);
  }

  document.body.classList.remove('windhawk-no-pointer-events');
}

function onOpenChange(open: boolean) {
  if (open) {
    document.body.classList.add('windhawk-no-pointer-events');
  } else {
    document.body.classList.remove('windhawk-no-pointer-events');
  }
}

const InputWithContextMenu = forwardRef<InputRef, InputProps>(
  ({ children, ...rest }, ref) => {
    const items = useItems();
    const internalRef = useRef<InputRef>(null);

    useImperativeHandle(ref, () => internalRef.current || ({} as InputRef));

    const handleMenuClick = useCallback(
      (info: { key: string }) => onClick(internalRef.current?.input || null, info.key),
      []
    );

    useEffect(() => {
      return () => {
        document.body.classList.remove('windhawk-no-pointer-events');
      };
    }, []);

    return (
      <Dropdown
        menu={{
          items,
          onClick: handleMenuClick,
        }}
        onOpenChange={onOpenChange}
        trigger={['contextMenu']}
        overlayClassName="windhawk-popup-content-no-select"
      >
        <Input ref={internalRef} {...rest}>
          {children}
        </Input>
      </Dropdown>
    );
  }
);

InputWithContextMenu.displayName = 'InputWithContextMenu';

const InputNumberWithContextMenu = forwardRef<HTMLInputElement, InputNumberProps>(
  ({ children, ...rest }, ref) => {
    const items = useItems();
    const internalRef = useRef<HTMLInputElement>(null);

    useImperativeHandle(ref, () => internalRef.current || ({} as HTMLInputElement));

    const handleMenuClick = useCallback(
      (info: { key: string }) => onClick(internalRef.current || null, info.key),
      []
    );

    useEffect(() => {
      return () => {
        document.body.classList.remove('windhawk-no-pointer-events');
      };
    }, []);

    return (
      <Dropdown
        menu={{
          items,
          onClick: handleMenuClick,
        }}
        onOpenChange={onOpenChange}
        trigger={['contextMenu']}
        overlayClassName="windhawk-popup-content-no-select"
      >
        <InputNumber ref={internalRef} {...rest}>
          {children}
        </InputNumber>
      </Dropdown>
    );
  }
);

InputNumberWithContextMenu.displayName = 'InputNumberWithContextMenu';

const TextAreaWithContextMenu = forwardRef<TextAreaRef, TextAreaProps>(
  ({ children, ...rest }, ref) => {
    const items = useItems();
    const internalRef = useRef<TextAreaRef>(null);

    useImperativeHandle(ref, () => internalRef.current || ({} as TextAreaRef));

    const handleMenuClick = useCallback(
      (info: { key: string }) =>
        onClick(internalRef.current?.resizableTextArea?.textArea || null, info.key),
      []
    );

    useEffect(() => {
      return () => {
        document.body.classList.remove('windhawk-no-pointer-events');
      };
    }, []);

    return (
      <Dropdown
        menu={{
          items,
          onClick: handleMenuClick,
        }}
        onOpenChange={onOpenChange}
        trigger={['contextMenu']}
        overlayClassName="windhawk-popup-content-no-select"
      >
        <Input.TextArea ref={internalRef} {...rest}>
          {children}
        </Input.TextArea>
      </Dropdown>
    );
  }
);

TextAreaWithContextMenu.displayName = 'TextAreaWithContextMenu';

function SelectModal({ children, ...rest }: SelectProps) {
  const handleDropdownVisibleChange = useCallback(
    (open: boolean) => {
      onOpenChange(open);
      rest.onDropdownVisibleChange?.(open);
    },
    [rest]
  );

  return (
    <Select
      popupClassName="windhawk-popup-content"
      {...rest}
      onDropdownVisibleChange={handleDropdownVisibleChange}
    >
      {children}
    </Select>
  );
}

function PopconfirmModal({ children, ...rest }: PopconfirmProps) {
  const handleOpenChange = useCallback(
    (open: boolean) => {
      onOpenChange(open);
      rest.onOpenChange?.(open);
    },
    [rest]
  );

  return (
    <Popconfirm
      overlayClassName="windhawk-popup-content"
      {...rest}
      onOpenChange={handleOpenChange}
    >
      {children}
    </Popconfirm>
  );
}

function DropdownModal({ children, ...rest }: DropdownProps) {
  const handleOpenChange = useCallback(
    (open: boolean) => {
      onOpenChange(open);
      rest.onOpenChange?.(open);
    },
    [rest]
  );

  return (
    <Dropdown
      {...rest}
      onOpenChange={handleOpenChange}
      overlayClassName="windhawk-popup-content-no-select"
    >
      {children}
    </Dropdown>
  );
}

function dropdownModalDismissed() {
  document.body.classList.remove('windhawk-no-pointer-events');
}

export {
  InputWithContextMenu,
  InputNumberWithContextMenu,
  TextAreaWithContextMenu,
  SelectModal,
  PopconfirmModal,
  DropdownModal,
  dropdownModalDismissed,
};
