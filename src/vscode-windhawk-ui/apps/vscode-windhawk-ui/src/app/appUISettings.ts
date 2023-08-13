import React from 'react';
import { AppUISettings } from './webviewIPCMessages';

export type AppUISettingsContextType = Partial<AppUISettings>;

export const AppUISettingsContext =
  React.createContext<AppUISettingsContextType>({});
