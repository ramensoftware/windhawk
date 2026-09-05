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
import { testIdProps } from '@app/utils';

/// #if EXTENSION && !TAURI
function useItems() {
  const { t } = useTranslation();

  const items: MenuProps['items'] = useMemo(
    () => [
      {
        label: t('general.contextMenu.cut'),
        key: 'cut',
      },
      {
        label: t('general.contextMenu.copy'),
        key: 'copy',
      },
      {
        label: t('general.contextMenu.paste'),
        key: 'paste',
      },
      {
        type: 'divider',
      },
      {
        label: t('general.contextMenu.selectAll'),
        key: 'selectAll',
      },
    ],
    [t]
  );

  return items;
}

// Why each wrapper below hands its own `disabled` to the Dropdown as well as to
// the field: antd's Dropdown clones its child with its own disabled prop, so a
// field told it is disabled comes back out enabled unless the wrapper is told
// too. It is the right thing to tell either way - there is nothing to cut, copy
// or paste in a field that cannot be edited.
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
        disabled={rest.disabled}
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
        disabled={rest.disabled}
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
        disabled={rest.disabled}
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
  const openOnWindowBlurRef = useRef(false);
  const containerRef = useRef<HTMLDivElement>(null);

  // Leaving the window takes the dropdown down with it: the container blur
  // rc-select gets from it closes the dropdown through a delayed callback, so
  // the close lands once the user is already elsewhere. Put it back when the
  // window returns to a Select that still holds focus.
  //
  // Only a dropdown that was open when the window lost focus is put back. A
  // Select keeps focus after a value is picked and after Escape closes the
  // dropdown, so focus alone says nothing about whether there was a dropdown to
  // restore, and a closed one would spring open over the form on every trip to
  // another window.
  useEffect(() => {
    const handleWindowBlur = () => {
      openOnWindowBlurRef.current = open;
    };
    const handleWindowFocus = () => {
      if (!openOnWindowBlurRef.current) {
        return;
      }
      openOnWindowBlurRef.current = false;
      setTimeout(() => {
        if (containerRef.current?.contains(document.activeElement)) {
          setOpen(true);
        }
      }, 100);
    };
    window.addEventListener('blur', handleWindowBlur);
    window.addEventListener('focus', handleWindowFocus);
    return () => {
      window.removeEventListener('blur', handleWindowBlur);
      window.removeEventListener('focus', handleWindowFocus);
    };
  }, [open]);

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
            // rc-select opens the dropdown from this very keydown, as soon as
            // this callback returns, so the block only has to hold for the rest
            // of the event - and must not hold past it. Most of the keystrokes
            // that get here open nothing for it to block, because rc-select
            // never opens on Ctrl, Alt or Meta on their own, nor on Tab,
            // Backspace, Escape or F1-F12, and a block left armed would be
            // spent on the user's next click on the Select instead.
            blockOpenRef.current = true;
            setTimeout(() => {
              blockOpenRef.current = false;
            }, 0);
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
    <Popconfirm
      {...rest}
      overlayClassName="windhawk-popup-content"
      okButtonProps={{ ...rest.okButtonProps, ...testIdProps('popconfirm-ok') }}
      cancelButtonProps={{
        ...rest.cancelButtonProps,
        ...testIdProps('popconfirm-cancel'),
      }}
    >
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
