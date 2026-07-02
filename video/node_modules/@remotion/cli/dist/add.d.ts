export declare const addCommand: ({ remotionRoot, packageManager, packageNames, logLevel, args, }: {
    remotionRoot: string;
    packageManager: string | undefined;
    packageNames: string[];
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    args: string[];
}) => Promise<void>;
