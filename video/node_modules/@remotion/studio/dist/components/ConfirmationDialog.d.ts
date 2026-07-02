import React from 'react';
import type { ConfirmationDialogState } from '../state/modals';
import type { ConfirmationDialogFunction } from './ConfirmationDialog-types';
export declare const useConfirmationDialog: () => ConfirmationDialogFunction;
export declare const ConfirmationDialog: React.FC<{
    readonly state: ConfirmationDialogState;
}>;
