/**
 * Gets the precompose rect for an element with a filter that requires precompositing.
 * Expands the element's bounding rect (including all children) to accommodate drop-shadow spread.
 */
export declare const getPrecomposeRectForFilter: ({ element, filter, }: {
    element: HTMLElement | SVGElement;
    filter: string;
}) => DOMRect;
/**
 * Applies a filter when drawing a precomposed canvas to the main context.
 */
export declare const applyFilterToDrawOperation: ({ context, filter, drawFn, }: {
    context: OffscreenCanvasRenderingContext2D;
    filter: string;
    drawFn: () => void;
}) => void;
