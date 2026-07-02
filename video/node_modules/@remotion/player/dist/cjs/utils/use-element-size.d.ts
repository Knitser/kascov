export type Size = {
    width: number;
    height: number;
    left: number;
    top: number;
    windowSize: {
        width: number;
        height: number;
    };
    refresh: () => void;
};
export declare const updateAllElementsSizes: () => void;
type ElementSizeSource = React.RefObject<HTMLElement | null> | HTMLElement | null;
export declare const useElementSize: (source: ElementSizeSource, options: {
    triggerOnWindowResize: boolean;
    shouldApplyCssTransforms: boolean;
}) => Size | null;
export {};
