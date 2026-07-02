export type Rgba = {
    r: number;
    g: number;
    b: number;
    a: number;
};
export type Hsva = {
    h: number;
    s: number;
    v: number;
    a: number;
};
export declare const clamp: (value: number, min: number, max: number) => number;
export declare const parseAnyColor: (input: string) => Rgba;
export declare const formatRgbHex: ({ r, g, b }: {
    r: number;
    g: number;
    b: number;
}) => string;
export declare const formatRgba: ({ r, g, b, a }: Rgba) => string;
export declare const rgbToHsv: ({ r, g, b, }: {
    r: number;
    g: number;
    b: number;
}) => Hsva;
export declare const hsvToRgb: ({ h, s, v, }: {
    h: number;
    s: number;
    v: number;
}) => {
    r: number;
    g: number;
    b: number;
};
export declare const rgbaToHsva: (rgba: Rgba) => Hsva;
export declare const hsvaToRgba: (hsva: Hsva) => Rgba;
