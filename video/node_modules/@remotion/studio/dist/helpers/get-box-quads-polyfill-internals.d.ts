/**
 * @param {globalThis} windowObj?
 * @param {boolean=} force
 */
export declare function addPolyfill(windowObj?: globalThis, force?: boolean | undefined): void;
/**
 * @param {globalThis} windowObj?
 */
export declare function patchAdoptNode(windowObj?: globalThis): void;
/**
 * @param {Node} node
 * @param {DOMQuadInit} quad
 * @param {Element} from
 * @param {{fromBox?: 'margin'|'border'|'padding'|'content', toBox?: 'margin'|'border'|'padding'|'content', iframes?: HTMLIFrameElement[]}=} options
 * @returns {DOMQuad}
 */
export declare function convertQuadFromNode(node: Node, quad: DOMQuadInit, from: Element, options?: {
    fromBox?: 'margin' | 'border' | 'padding' | 'content';
    toBox?: 'margin' | 'border' | 'padding' | 'content';
    iframes?: HTMLIFrameElement[];
} | undefined): DOMQuad;
/**
 * @param {Node} node
 * @param {{x: number, y: number, width: number, height: number}} rect
 * @param {Element} from
 * @param {{fromBox?: 'margin'|'border'|'padding'|'content', toBox?: 'margin'|'border'|'padding'|'content', iframes?: HTMLIFrameElement[]}=} options
 * @returns {DOMQuad}
 */
export declare function convertRectFromNode(node: Node, rect: {
    x: number;
    y: number;
    width: number;
    height: number;
}, from: Element, options?: {
    fromBox?: 'margin' | 'border' | 'padding' | 'content';
    toBox?: 'margin' | 'border' | 'padding' | 'content';
    iframes?: HTMLIFrameElement[];
} | undefined): DOMQuad;
/**
 * @param {Node} node
 * @param {DOMPointInit} point
 * @param {Element} from
 * @param {{fromBox?: 'margin'|'border'|'padding'|'content', toBox?: 'margin'|'border'|'padding'|'content', iframes?: HTMLIFrameElement[]}=} options
 * @returns {DOMPoint}
 */
export declare function convertPointFromNode(node: Node, point: DOMPointInit, from: Element, options?: {
    fromBox?: 'margin' | 'border' | 'padding' | 'content';
    toBox?: 'margin' | 'border' | 'padding' | 'content';
    iframes?: HTMLIFrameElement[];
} | undefined): DOMPoint;
export declare function clearCache(): void;
export declare function useCache(): void;
/**
 * @param {Node} node
 * @param {{box?: 'margin'|'border'|'padding'|'content', relativeTo?: Element, iframes?: HTMLIFrameElement[]}=} options
 * @returns {DOMQuad[]}
 */
export declare function getBoxQuads(node: Node, options?: {
    box?: 'margin' | 'border' | 'padding' | 'content';
    relativeTo?: Element;
    iframes?: HTMLIFrameElement[];
} | undefined): DOMQuad[];
/**
 * @param {Node} node
 * @param {DOMMatrix=} matrix
 */
export declare function getElementSize(node: Node, matrix?: DOMMatrix | undefined): {
    width: number;
    height: number;
};
/**
 * @param {Node} node
 * @param {Element} ancestor
 * @param {HTMLIFrameElement[]} iframes
 * @param {boolean=} excludeSelfZoom
 */
export declare function getResultingTransformationBetweenElementAndAllAncestors(node: Node, ancestor: Element, iframes: HTMLIFrameElement[], excludeSelfZoom?: boolean | undefined): DOMMatrix;
/**
 * @param {Node} element
 * @param {HTMLIFrameElement[]=} iframes
 */
export declare function getElementCombinedTransform(element: Node, iframes?: HTMLIFrameElement[] | undefined): DOMMatrix;
