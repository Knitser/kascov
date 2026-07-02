/**
 * When the Node parent dies abruptly (SIGKILL, kernel OOM, etc.), child processes
 * are reparented to init and may keep running. On Linux, `setpriv(1)` uses
 * prctl(PR_SET_PDEATHSIG) so the child receives SIGKILL when its parent dies.
 *
 * @see https://github.com/remotion-dev/remotion/issues/7207
 */
export declare const resolveSetprivPathOnLinux: () => string | null;
export declare const wrapExecutableWithSetprivIfAvailable: ({ executablePath, args, }: {
    executablePath: string;
    args: string[];
}) => {
    executablePath: string;
    args: string[];
};
