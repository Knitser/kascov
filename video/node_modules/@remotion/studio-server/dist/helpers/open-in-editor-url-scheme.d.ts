export declare const openInEditorViaUrlScheme: ({ editor, fileName, lineNumber, colNumber, }: {
    editor: string;
    fileName: string;
    lineNumber: number;
    colNumber: number;
}) => Promise<boolean> | null;
