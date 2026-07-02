import type { PackageManager } from '@remotion/studio-shared';
type LockfilePath = {
    manager: PackageManager;
    path: string;
    installCommand: string;
    startCommand: string;
};
export declare const lockFilePaths: LockfilePath[];
export declare const getPackageManager: ({ remotionRoot, packageManager, dirUp, logLevel, }: {
    remotionRoot: string;
    packageManager: string | undefined;
    dirUp: number;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
}) => "unknown" | LockfilePath;
export {};
