export declare const determineFinalStillImageFormat: ({ downloadName, outName, configuredImageFormat, isLambda, fromUi, }: {
    downloadName: string | null;
    outName: string | null;
    configuredImageFormat: "jpeg" | "pdf" | "png" | "webp" | null;
    isLambda: boolean;
    fromUi: "jpeg" | "pdf" | "png" | "webp" | null;
}) => {
    format: "jpeg" | "pdf" | "png" | "webp";
    source: string;
};
