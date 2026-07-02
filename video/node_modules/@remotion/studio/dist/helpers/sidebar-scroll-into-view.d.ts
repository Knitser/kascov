export declare const scrollSidebarRowIntoViewIfNeeded: (element: HTMLElement | null) => void;
export declare const markCompositionSidebarScrollFromRowClick: (compositionId: string) => void;
export declare const maybeScrollCompositionSidebarRowIntoView: ({ element, compositionId, selected, }: {
    element: HTMLElement | null;
    compositionId: string;
    selected: boolean;
}) => void;
export declare const markAssetSidebarScrollFromRowClick: (assetPath: string) => void;
export declare const maybeScrollAssetSidebarRowIntoView: ({ element, assetPath, selected, }: {
    element: HTMLElement | null;
    assetPath: string;
    selected: boolean;
}) => void;
