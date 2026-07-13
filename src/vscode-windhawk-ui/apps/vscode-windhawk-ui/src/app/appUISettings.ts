import React from 'react';
import { type AppUISettings } from './webviewIPCMessages';

export type AppUISettingsContextType = Partial<AppUISettings>;

export const AppUISettingsContext =
  React.createContext<AppUISettingsContextType>({});
