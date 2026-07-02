import React from 'react';
type Options = {
    dontThrowIfOutsideOfRemotion?: boolean;
};
export declare const PixelDensityContext: React.Context<number | null>;
export declare const usePixelDensity: (options?: Options | undefined) => number;
export {};
