export declare const getUserPassedOutputLocation: (args: (string | number)[], uiPassedOutName: string | null) => string | null;
export declare const getOutputLocation: ({ compositionId, defaultExtension, args, type, outputLocationFromUi, compositionDefaultOutName, }: {
    compositionId: string;
    outputLocationFromUi: string | null;
    compositionDefaultOutName: string | null;
    defaultExtension: string;
    args: (string | number)[];
    type: "asset" | "sequence";
}) => string;
