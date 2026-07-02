import type { PropsEditType } from './DataEditor';
export declare const CANNOT_SAVE_DEFAULT_PROPS_DOCS = "https://www.remotion.dev/docs/troubleshooting/cannot-save-default-props";
export type RenderModalWarning = {
    readonly id: string;
    readonly message: string;
    readonly resolveLink?: string;
};
export type TypeCanSaveState = {
    canUpdate: true;
} | {
    canUpdate: false;
    reason: string;
    determined: boolean;
};
export declare const getRenderModalWarnings: ({ cliProps, canSaveDefaultProps, isCustomDateUsed, customFileUsed, jsMapUsed, jsSetUsed, inJSONEditor, propsEditType, showCannotSaveDefaultPropsWarning, }: {
    cliProps: unknown;
    canSaveDefaultProps: TypeCanSaveState | null;
    isCustomDateUsed: boolean;
    customFileUsed: boolean;
    jsMapUsed: boolean;
    jsSetUsed: boolean;
    inJSONEditor: boolean;
    propsEditType: PropsEditType;
    showCannotSaveDefaultPropsWarning: boolean;
}) => RenderModalWarning[];
