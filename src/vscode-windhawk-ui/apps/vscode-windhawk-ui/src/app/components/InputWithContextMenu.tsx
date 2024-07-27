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
import { useMemo, useRef } from 'react';
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

function InputWithContextMenu({ children, ...rest }: InputProps) {
  const items = useItems();
  const ref = useRef<InputRef>(null);
  return (
    <Dropdown
      menu={{
        items,
        onClick: (info) => onClick(ref.current?.input, info.key),
      }}
      onOpenChange={onOpenChange}
      trigger={['contextMenu']}
      overlayClassName="windhawk-popup-content-no-select"
    >
      <Input ref={ref} {...rest}>
        {children}
      </Input>
    </Dropdown>
  );
}

function InputNumberWithContextMenu({ children, ...rest }: InputNumberProps) {
  const items = useItems();
  const ref = useRef<HTMLInputElement>(null);
  return (
    <Dropdown
      menu={{
        items,
        onClick: (info) => onClick(ref.current, info.key),
      }}
      onOpenChange={onOpenChange}
      trigger={['contextMenu']}
      overlayClassName="windhawk-popup-content-no-select"
    >
      <InputNumber ref={ref} {...rest}>
        {children}
      </InputNumber>
    </Dropdown>
  );
}

function TextAreaWithContextMenu({ children, ...rest }: TextAreaProps) {
  const items = useItems();
  const ref = useRef<TextAreaRef>(null);
  return (
    <Dropdown
      menu={{
        items,
        onClick: (info) =>
          onClick(ref.current?.resizableTextArea?.textArea, info.key),
      }}
      onOpenChange={onOpenChange}
      trigger={['contextMenu']}
      overlayClassName="windhawk-popup-content-no-select"
    >
      <Input.TextArea ref={ref} {...rest}>
        {children}
      </Input.TextArea>
    </Dropdown>
  );
}

function SelectModal({ children, ...rest }: SelectProps) {
  return (
    <Select
      popupClassName="windhawk-popup-content"
      {...rest}
      onDropdownVisibleChange={(open) => {
        onOpenChange(open);
        rest.onDropdownVisibleChange?.(open);
      }}
    >
      {children}
    </Select>
  );
}

function PopconfirmModal({ children, ...rest }: PopconfirmProps) {
  return (
    <Popconfirm
      overlayClassName="windhawk-popup-content"
      {...rest}
      onOpenChange={(open) => {
        onOpenChange(open);
        rest.onOpenChange?.(open);
      }}
    >
      {children}
    </Popconfirm>
  );
}

function DropdownModal({ children, ...rest }: DropdownProps) {
  return (
    <Dropdown
      {...rest}
      onOpenChange={(open) => {
        onOpenChange(open);
        rest.onOpenChange?.(open);
      }}
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
