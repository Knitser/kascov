import React from 'react';
export declare const SaturationValueArea: React.FC<{
    readonly hue: number;
    readonly saturation: number;
    readonly value: number;
    readonly onChange: (next: {
        s: number;
        v: number;
    }) => void;
    readonly onChangeComplete: (next: {
        s: number;
        v: number;
    }) => void;
}>;
