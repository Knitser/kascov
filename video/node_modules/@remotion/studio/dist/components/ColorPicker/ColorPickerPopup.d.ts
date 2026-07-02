import React from 'react';
export declare const POPUP_WIDTH = 240;
export declare const hasEyeDropper: () => boolean;
export declare const parseEyeDropperColor: (pickedColor: string) => {
    r: number;
    g: number;
    b: number;
    a: number;
};
export type ChannelKey = 'r' | 'g' | 'b' | 'a-percent';
export declare const ColorPickerPopup: React.FC<{
    readonly value: string;
    readonly onChange: (next: string) => void;
    readonly onChangeComplete: (next: string) => void;
}>;
