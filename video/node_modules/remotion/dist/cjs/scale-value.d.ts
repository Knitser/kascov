export type ScaleValue = readonly [number, number, number];
export declare const parseValidScaleValue: (value: unknown) => ScaleValue | null;
export declare const parseScaleValue: (value: unknown) => ScaleValue;
export declare const serializeScaleValue: ([x, y, z]: ScaleValue) => string | number;
