import React from 'react';
export declare const AlphaSlider: React.FC<{
    readonly hue: number;
    readonly saturation: number;
    readonly value: number;
    readonly alpha: number;
    readonly onChange: (next: number) => void;
    readonly onChangeComplete: (next: number) => void;
}>;
