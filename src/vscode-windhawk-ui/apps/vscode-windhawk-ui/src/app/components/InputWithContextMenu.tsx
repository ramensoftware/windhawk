import {
  Dropdown,
  type DropdownProps,
  Input,
  InputNumber,
  type InputNumberProps,
  type MenuProps,
  Popconfirm,
  type PopconfirmProps,
  Select,
  type SelectProps,
} from 'antd';
import { type InputProps, type InputRef, type TextAreaProps } from 'antd/lib/input';
import { type TextAreaRef } from 'antd/lib/input/TextArea';
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

/// #if EXTENSION && !TAURI
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
}

const InputWithContextMenuExtension = forwardRef<InputRef, InputProps>(
  ({ children, ...rest }, ref) => {
    const items = useItems();
    const internalRef = useRef<InputRef>(null);

    useImperativeHandle(ref, () => internalRef.current || ({} as InputRef));

    const handleMenuClick = useCallback(
      (info: { key: string }) => onClick(internalRef.current?.input || null, info.key),
      []
    );

    return (
      <Dropdown
        menu={{
          items,
          onClick: handleMenuClick,
        }}
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

InputWithContextMenuExtension.displayName = 'InputWithContextMenu';

const InputNumberWithContextMenuExtension = forwardRef<HTMLInputElement, InputNumberProps>(
  ({ children, ...rest }, ref) => {
    const items = useItems();
    const internalRef = useRef<HTMLInputElement>(null);

    useImperativeHandle(ref, () => internalRef.current || ({} as HTMLInputElement));

    const handleMenuClick = useCallback(
      (info: { key: string }) => onClick(internalRef.current || null, info.key),
      []
    );

    return (
      <Dropdown
        menu={{
          items,
          onClick: handleMenuClick,
        }}
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

InputNumberWithContextMenuExtension.displayName = 'InputNumberWithContextMenu';

const TextAreaWithContextMenuExtension = forwardRef<TextAreaRef, TextAreaProps>(
  ({ children, ...rest }, ref) => {
    const items = useItems();
    const internalRef = useRef<TextAreaRef>(null);

    useImperativeHandle(ref, () => internalRef.current || ({} as TextAreaRef));

    const handleMenuClick = useCallback(
      (info: { key: string }) =>
        onClick(internalRef.current?.resizableTextArea?.textArea || null, info.key),
      []
    );

    return (
      <Dropdown
        menu={{
          items,
          onClick: handleMenuClick,
        }}
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

TextAreaWithContextMenuExtension.displayName = 'TextAreaWithContextMenu';
/// #endif

declare const WEBPACK_IS_WEBSITE: boolean;
declare const WEBPACK_IS_TAURI: boolean;

// The custom context menu relies on document.execCommand and the VSCode webview
// host. The website has no such host, and the Tauri shell provides its own native
// edit context menu, so both fall back to the plain antd inputs.
const useNativeContextMenu = WEBPACK_IS_WEBSITE || WEBPACK_IS_TAURI;

const InputWithContextMenu = useNativeContextMenu
  ? Input
  : InputWithContextMenuExtension;
const InputNumberWithContextMenu = useNativeContextMenu
  ? InputNumber
  : InputNumberWithContextMenuExtension;
const TextAreaWithContextMenu = useNativeContextMenu
  ? Input.TextArea
  : TextAreaWithContextMenuExtension;

function SelectModal({ children, ...rest }: Omit<SelectProps, 'open' | 'popupClassName'>) {
  // Prevent dropdown from opening on keyboard shortcuts (Ctrl+S, Ctrl+A, etc.)
  // by using controlled open state and blocking when modifier keys are pressed.
  const [open, setOpen] = useState(false);
  const blockOpenRef = useRef(false);
  const containerRef = useRef<HTMLDivElement>(null);

  // rc-select's onContainerBlur uses a 10ms delayed callback to call
  // onToggleOpen(false). If the window regains focus within that window,
  // onContainerFocus cancels the pending timeout, so the close never fires.
  // This leaves our controlled `open` stuck at true while the popup is hidden,
  // causing the next click to be swallowed. Re-open the dropdown on window
  // focus if the Select's input is still focused, so the state matches reality.
  useEffect(() => {
    const handleWindowFocus = () => {
      setTimeout(() => {
        if (containerRef.current?.contains(document.activeElement)) {
          setOpen(true);
        }
      }, 100);
    };
    window.addEventListener('focus', handleWindowFocus);
    return () => {
      window.removeEventListener('focus', handleWindowFocus);
    };
  }, []);

  return (
    <div ref={containerRef}>
      <Select
        {...rest}
        popupClassName="windhawk-popup-content"
        open={open}
        onDropdownVisibleChange={(o) => {
          if (o && blockOpenRef.current) {
            blockOpenRef.current = false;
            return;
          }

          // The timeout is a workaround for the following issue: when the
          // window is unfocused while the dropdown is focused, and is clicked
          // with the mouse, it fails to open the dropdown.
          setTimeout(() => {
            setOpen(o);
            rest.onDropdownVisibleChange?.(o);
          }, 20);
        }}
        onInputKeyDown={(e) => {
          if (e.ctrlKey || e.altKey || e.metaKey) {
            blockOpenRef.current = true;
          }
          rest.onInputKeyDown?.(e);
        }}
      >
        {children}
      </Select>
    </div>
  );
}

function PopconfirmModal({ children, ...rest }: Omit<PopconfirmProps, 'overlayClassName'>) {
  return (
    <Popconfirm {...rest} overlayClassName="windhawk-popup-content">
      {children}
    </Popconfirm>
  );
}

function DropdownModal({ children, ...rest }: Omit<DropdownProps, 'overlayClassName'>) {
  return (
    <Dropdown {...rest} overlayClassName="windhawk-popup-content-no-select">
      {children}
    </Dropdown>
  );
}

export {
  InputWithContextMenu,
  InputNumberWithContextMenu,
  TextAreaWithContextMenu,
  SelectModal,
  PopconfirmModal,
  DropdownModal,
};
