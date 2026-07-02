import React from 'react';
import type { TypeCanSaveState } from './RenderModal/get-render-modal-warnings';
export type TObserveDefaultPropsContext = {
    canSaveDefaultProps: TypeCanSaveState | null;
};
export declare const ObserveDefaultPropsContext: React.Context<TObserveDefaultPropsContext | null>;
export declare const ObserveDefaultProps: React.FC<{
    readonly compositionId: string | null;
    readonly readOnlyStudio: boolean;
    readonly children: React.ReactNode;
}>;
