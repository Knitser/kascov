import React from 'react';
export declare const HueSlider: React.FC<{
    readonly hue: number;
    readonly onChange: (next: number) => void;
    readonly onChangeComplete: (next: number) => void;
}>;
