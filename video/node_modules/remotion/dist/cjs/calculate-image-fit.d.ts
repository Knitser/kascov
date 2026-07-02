export type ImageFit = 'contain' | 'cover' | 'fill';
export declare const calculateImageFit: (fit: ImageFit, imageSize: {
    width: number;
    height: number;
}, canvasSize: {
    width: number;
    height: number;
}) => [number, number, number, number, number, number, number, number];
