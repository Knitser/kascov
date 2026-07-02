/**
 * Copied from:
 * https://github.com/software-mansion/react-native-reanimated/blob/master/src/reanimated2/Colors.ts
 */
import { type InterpolateOptions } from './interpolate.js';
export type InterpolateColorsOptions = Pick<InterpolateOptions, 'easing' | 'posterize'>;
export declare const colorNames: {
    [key: string]: number;
};
export declare function processColor(color: string): number;
export declare const interpolateColors: (input: number, inputRange: readonly number[], outputRange: readonly string[], options?: InterpolateColorsOptions | undefined) => string;
